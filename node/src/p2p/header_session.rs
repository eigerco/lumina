use celestia_proto::p2p::pb::HeaderRequest;
use celestia_types::ExtendedHeader;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::executor::spawn;
use crate::p2p::header_ex::utils::HeaderRequestExt;
use crate::p2p::{HeaderExError, P2pCmd, P2pError};
use crate::store::header_ranges::{
    HeaderRanges, HeaderRangesExt, HeaderRangesIterator, RangeLengthExt,
};

const MAX_AMOUNT_PER_REQ: u64 = 64;
const MAX_CONCURRENT_REQS: usize = 1;

type Result<T, E = P2pError> = std::result::Result<T, E>;

pub(crate) struct HeaderSession {
    ranges_iter: HeaderRangesIterator,
    ranges_count: usize,
    cmd_tx: mpsc::Sender<P2pCmd>,
    response_tx: mpsc::Sender<(u64, u64, Result<Vec<ExtendedHeader>>)>,
    response_rx: mpsc::Receiver<(u64, u64, Result<Vec<ExtendedHeader>>)>,
    ongoing: usize,
}

impl HeaderSession {
    /// Create a new HeaderSession responsible for fetching provided `ranges` headers.
    /// `HeaderRanges` can be created manually, or more probably using
    /// [`Store::get_stored_header_ranges`] to fetch existing header ranges and then using
    /// [`calculate_missing_ranges`] to convert that into ranges of headers that are missing.
    /// Received headers are sent over `cmd_tx` as a vector of contiguous header ranges, e.g.
    /// for requested ranges
    /// `[1..=3, 6..=9]`
    /// response would be
    /// `vec![vec![1, 2, 3], vec![6, 7, 8, 9]]`
    ///
    /// [`calculate_missing_ranges`]: crate::store::utils::calculate_missing_ranges
    /// [`Store::get_stored_header_ranges`]: crate::store::Store::get_stored_header_ranges
    pub(crate) fn new(ranges: HeaderRanges, cmd_tx: mpsc::Sender<P2pCmd>) -> Result<Self> {
        ranges.validate().map_err(|e| {
            warn!("invalid header range: {e:?}");
            HeaderExError::InvalidRequest
        })?;

        let ranges_count = ranges.as_ref().len();
        let (response_tx, response_rx) = mpsc::channel(MAX_CONCURRENT_REQS);

        Ok(HeaderSession {
            ranges_iter: ranges.into_iter(),
            ranges_count,
            cmd_tx,
            response_tx,
            response_rx,
            ongoing: 0,
        })
    }

    pub(crate) async fn run(&mut self) -> Result<Vec<Vec<ExtendedHeader>>> {
        let mut responses = Vec::new();

        for _ in 0..MAX_CONCURRENT_REQS {
            self.send_next_request().await?;
        }

        while self.ongoing > 0 {
            let (height, requested_amount, res) = self.recv_response().await;

            match res {
                Ok(headers) => {
                    let headers_len = headers.len() as u64;

                    if headers_len > 0 {
                        responses.push(headers);
                    }

                    if headers_len < requested_amount {
                        // Reschedule the missing sub-range
                        let height = height + headers_len;
                        let amount = requested_amount - headers_len;
                        self.send_request(height, amount).await?;

                        debug!("requested {requested_amount}, got {headers_len}: retrying {height} +{amount}");
                    } else {
                        // Schedule next request
                        self.send_next_request().await?;
                    }
                }
                Err(P2pError::HeaderEx(e)) => {
                    debug!("HeaderEx error: {e}");
                    self.send_request(height, requested_amount).await?;
                }
                Err(e) => return Err(e),
            }
        }

        let ranges = sort_and_flatten_header_ranges(responses, self.ranges_count);

        Ok(ranges)
    }

    async fn recv_response(&mut self) -> (u64, u64, Result<Vec<ExtendedHeader>>) {
        let (height, requested_amount, res) =
            self.response_rx.recv().await.expect("channel never closes");

        self.ongoing -= 1;

        (height, requested_amount, res)
    }

    pub(crate) async fn send_next_request(&mut self) -> Result<()> {
        let Some(range) = self.ranges_iter.next_batch(MAX_AMOUNT_PER_REQ) else {
            return Ok(());
        };

        self.send_request(*range.start(), range.len()).await?;

        Ok(())
    }

    pub(crate) async fn send_request(&mut self, height: u64, amount: u64) -> Result<()> {
        debug!("Fetching batch {} until {}", height, height + amount - 1);

        let request = HeaderRequest::with_origin(height, amount);
        let (tx, rx) = oneshot::channel();

        self.cmd_tx
            .send(P2pCmd::HeaderExRequest {
                request,
                respond_to: tx,
            })
            .await
            .map_err(|_| P2pError::WorkerDied)?;

        let response_tx = self.response_tx.clone();

        spawn(async move {
            let result = match rx.await {
                Ok(result) => result,
                Err(_) => Err(P2pError::WorkerDied),
            };
            let _ = response_tx.send((height, amount, result)).await;
        });

        self.ongoing += 1;

        Ok(())
    }
}

/// Given a vector of header spans which are internally sorted, return a vector of header ranges
/// with contiguous spans merged. For example input with following header heights
/// [[1, 2, 3], [6, 7], \[4\], [8, 9]]
/// will return
/// [[1, 2, 3, 4], [6, 7, 8, 9]]
fn sort_and_flatten_header_ranges(
    mut header_spans: Vec<Vec<ExtendedHeader>>,
    ranges_count: usize,
) -> Vec<Vec<ExtendedHeader>> {
    header_spans.sort_unstable_by_key(|span| {
        debug_assert!(!span.is_empty());
        span.first().unwrap().height().value()
    });

    let mut ranges = Vec::with_capacity(ranges_count);
    for mut span in header_spans {
        let Some(last_range) = ranges.last_mut() else {
            ranges.push(span);
            continue;
        };

        // we know that current and previously inserted spans aren't empty, unwraps are safe
        if last_range.last().unwrap().height().value() + 1 == span.first().unwrap().height().value()
        {
            last_range.append(&mut span);
        } else {
            ranges.push(span);
        }
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::{HeaderExError, P2p};
    use crate::test_utils::async_test;
    use celestia_types::test_utils::ExtendedHeaderGenerator;

    #[async_test]
    async fn retry_on_missing_range() {
        let (_p2p, mut p2p_mock) = P2p::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(64);

        let mut session = HeaderSession::new([1..=64].into(), p2p_mock.cmd_tx.clone()).unwrap();
        let (result_tx, result_rx) = oneshot::channel();
        spawn(async move {
            let res = session.run().await;
            result_tx.send(res).unwrap();
        });

        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 1);
        assert_eq!(amount, 64);
        respond_to.send(Ok(headers[..60].to_vec())).unwrap();

        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 61);
        assert_eq!(amount, 4);
        respond_to.send(Ok(headers[60..64].to_vec())).unwrap();

        p2p_mock.expect_no_cmd().await;

        let received_headers = result_rx.await.unwrap().unwrap();
        assert_eq!(vec![headers], received_headers);
    }

    #[async_test]
    async fn nine_batches() {
        let (_p2p, mut p2p_mock) = P2p::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(520);

        let mut session = HeaderSession::new([1..=520].into(), p2p_mock.cmd_tx.clone()).unwrap();
        let (result_tx, result_rx) = oneshot::channel();
        spawn(async move {
            let res = session.run().await;
            result_tx.send(res).unwrap();
        });

        for i in 0..8 {
            let (height, amount, respond_to) =
                p2p_mock.expect_header_request_for_height_cmd().await;
            assert_eq!(height, 1 + 64 * i);
            assert_eq!(amount, 64);
            let start = (height - 1) as usize;
            let end = start + amount as usize;
            respond_to.send(Ok(headers[start..end].to_vec())).unwrap();
        }

        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 513);
        assert_eq!(amount, 8);
        let start = (height - 1) as usize;
        let end = start + amount as usize;
        respond_to.send(Ok(headers[start..end].to_vec())).unwrap();

        p2p_mock.expect_no_cmd().await;

        let received_headers = result_rx.await.unwrap().unwrap();
        assert_eq!(vec![headers], received_headers);
    }

    #[async_test]
    async fn not_found_is_not_fatal() {
        let (_p2p, mut p2p_mock) = P2p::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(64);

        let mut session = HeaderSession::new([1..=64].into(), p2p_mock.cmd_tx.clone()).unwrap();
        let (result_tx, result_rx) = oneshot::channel();
        spawn(async move {
            let res = session.run().await;
            result_tx.send(res).unwrap();
        });

        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 1);
        assert_eq!(amount, 64);
        respond_to
            .send(Err(P2pError::HeaderEx(HeaderExError::HeaderNotFound)))
            .unwrap();

        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 1);
        assert_eq!(amount, 64);
        respond_to.send(Ok(headers.clone())).unwrap();

        p2p_mock.expect_no_cmd().await;

        let received_headers = result_rx.await.unwrap().unwrap();
        assert_eq!(vec![headers], received_headers);
    }

    #[async_test]
    async fn no_peers_is_fatal() {
        let (_p2p, mut p2p_mock) = P2p::mocked();

        let mut session = HeaderSession::new([1..=64].into(), p2p_mock.cmd_tx.clone()).unwrap();
        let (result_tx, result_rx) = oneshot::channel();
        spawn(async move {
            let res = session.run().await;
            result_tx.send(res).unwrap();
        });

        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 1);
        assert_eq!(amount, 64);
        respond_to.send(Err(P2pError::NoConnectedPeers)).unwrap();

        p2p_mock.expect_no_cmd().await;

        assert!(matches!(
            result_rx.await,
            Ok(Err(P2pError::NoConnectedPeers))
        ));
    }

    #[test]
    fn test_da_sort_headers_test() {
        let mut gen = ExtendedHeaderGenerator::new();
        let span0 = gen.next_many(10);
        gen.skip(10);
        let mut span1 = gen.next_many(10);
        let mut span2 = gen.next_many(10);
        gen.skip(10);
        let span3 = gen.next_many(10);

        let spans = vec![span0.clone(), span1.clone(), span2.clone(), span3.clone()];

        let ranges = sort_and_flatten_header_ranges(spans, 3);

        span1.append(&mut span2);
        assert_eq!(ranges[0], span0);
        assert_eq!(ranges[1], span1);
        assert_eq!(ranges[2], span3);
    }
}
