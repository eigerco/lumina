use celestia_proto::p2p::pb::HeaderRequest;
use celestia_types::ExtendedHeader;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use crate::executor::spawn;
use crate::p2p::header_ex::utils::HeaderRequestExt;
use crate::p2p::{P2pCmd, P2pError};
use crate::block_ranges::{BlockRange, BlockRangeExt};

const MAX_AMOUNT_PER_REQ: u64 = 64;
const MAX_CONCURRENT_REQS: usize = 8;

type Result<T, E = P2pError> = std::result::Result<T, E>;

pub(crate) struct HeaderSession {
    to_fetch: Option<BlockRange>,
    cmd_tx: mpsc::Sender<P2pCmd>,
    response_tx: mpsc::Sender<(u64, u64, Result<Vec<ExtendedHeader>>)>,
    response_rx: mpsc::Receiver<(u64, u64, Result<Vec<ExtendedHeader>>)>,
    ongoing: usize,
}

impl HeaderSession {
    /// Create a new HeaderSession responsible for fetching provided range of headers.
    /// `BlockRange` can be created manually, or more probably using
    /// [`Store::get_stored_header_ranges`] to fetch existing header ranges and then using
    /// [`calculate_fetch_range`] to return a first range that should be fetched.
    /// Received headers range is sent over `cmd_tx` as a vector of unverified headers.
    ///
    /// [`calculate_fetch_range`] crate::store::utils::calculate_fetch_range
    /// [`Store::get_stored_header_ranges`]: crate::store::Store::get_stored_header_ranges
    pub(crate) fn new(range: BlockRange, cmd_tx: mpsc::Sender<P2pCmd>) -> Self {
        let (response_tx, response_rx) = mpsc::channel(MAX_CONCURRENT_REQS);

        HeaderSession {
            to_fetch: Some(range),
            cmd_tx,
            response_tx,
            response_rx,
            ongoing: 0,
        }
    }

    pub(crate) async fn run(&mut self) -> Result<Vec<ExtendedHeader>> {
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

        responses.sort_unstable_by_key(|span| {
            span.first()
                .expect("empty spans aren't added in receiving loop")
                .height()
                .value()
        });

        Ok(responses.into_iter().flatten().collect())
    }

    async fn recv_response(&mut self) -> (u64, u64, Result<Vec<ExtendedHeader>>) {
        let (height, requested_amount, res) =
            self.response_rx.recv().await.expect("channel never closes");

        self.ongoing -= 1;

        (height, requested_amount, res)
    }

    pub(crate) async fn send_next_request(&mut self) -> Result<()> {
        let Some(range) = take_next_batch(&mut self.to_fetch, MAX_AMOUNT_PER_REQ) else {
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

/// take a next batch of up to `limit` headers from the front of the `range_to_fetch`
fn take_next_batch(range_to_fetch: &mut Option<BlockRange>, limit: u64) -> Option<BlockRange> {
    // calculate potential end offset before we modify range_to_fetch
    let end_offset = limit.checked_sub(1)?;

    let to_fetch = range_to_fetch.take()?;
    if to_fetch.len() <= limit {
        Some(to_fetch)
    } else {
        // to_fetch.len() > limit, we shouldn't underflow here
        let _ = range_to_fetch.insert(*to_fetch.start()..=*to_fetch.end() - limit);
        Some(*to_fetch.end() - end_offset..=*to_fetch.end())
    }
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

        let mut session = HeaderSession::new(1..=64, p2p_mock.cmd_tx.clone());
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
        assert_eq!(headers, received_headers);
    }

    #[async_test]
    async fn nine_batches() {
        let (_p2p, mut p2p_mock) = P2p::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(520);

        let mut session = HeaderSession::new(1..=520, p2p_mock.cmd_tx.clone());
        let (result_tx, result_rx) = oneshot::channel();
        spawn(async move {
            let res = session.run().await;
            result_tx.send(res).unwrap();
        });

        for i in (0..8).rev() {
            let (height, amount, respond_to) =
                p2p_mock.expect_header_request_for_height_cmd().await;
            assert_eq!(height, 9 + 64 * i);
            assert_eq!(amount, 64);
            let start = (height - 1) as usize;
            let end = start + amount as usize;
            respond_to.send(Ok(headers[start..end].to_vec())).unwrap();
        }

        let (height, amount, respond_to) = p2p_mock.expect_header_request_for_height_cmd().await;
        assert_eq!(height, 1);
        assert_eq!(amount, 8);
        let start = (height - 1) as usize;
        let end = start + amount as usize;
        respond_to.send(Ok(headers[start..end].to_vec())).unwrap();

        p2p_mock.expect_no_cmd().await;

        let received_headers = result_rx.await.unwrap().unwrap();
        assert_eq!(headers, received_headers);
    }

    #[async_test]
    async fn not_found_is_not_fatal() {
        let (_p2p, mut p2p_mock) = P2p::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(64);

        let mut session = HeaderSession::new(1..=64, p2p_mock.cmd_tx.clone());
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
        assert_eq!(headers, received_headers);
    }

    #[async_test]
    async fn no_peers_is_fatal() {
        let (_p2p, mut p2p_mock) = P2p::mocked();

        let mut session = HeaderSession::new(1..=64, p2p_mock.cmd_tx.clone());
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
    fn take_next_batch_full_batch() {
        let mut range_to_fetch = Some(1..=10);
        let batch = take_next_batch(&mut range_to_fetch, 16);
        assert_eq!(batch, Some(1..=10));
        assert_eq!(range_to_fetch, None);
    }

    #[test]
    fn take_next_batch_equal_limit() {
        let mut range_to_fetch = Some(1..=10);
        let batch = take_next_batch(&mut range_to_fetch, 10);
        assert_eq!(batch, Some(1..=10));
        assert_eq!(range_to_fetch, None);
    }

    #[test]
    fn take_next_batch_truncated_batch() {
        let mut range_to_fetch = Some(1..=10);
        let batch = take_next_batch(&mut range_to_fetch, 5);
        assert_eq!(batch, Some(6..=10));
        assert_eq!(range_to_fetch, Some(1..=5));
    }

    #[test]
    fn take_next_batch_truncated_calc() {
        let mut range_to_fetch = Some(1..=512);

        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(449..=512));
        assert_eq!(range_to_fetch, Some(1..=448));
        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(385..=448));
        assert_eq!(range_to_fetch, Some(1..=384));
        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(321..=384));
        assert_eq!(range_to_fetch, Some(1..=320));
        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(257..=320));
        assert_eq!(range_to_fetch, Some(1..=256));
        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(193..=256));
        assert_eq!(range_to_fetch, Some(1..=192));
        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(129..=192));
        assert_eq!(range_to_fetch, Some(1..=128));
        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(65..=128));
        assert_eq!(range_to_fetch, Some(1..=64));
        let batch = take_next_batch(&mut range_to_fetch, 64);
        assert_eq!(batch, Some(1..=64));
        assert_eq!(range_to_fetch, None);
    }

    #[test]
    fn take_next_batch_none() {
        let mut range_to_fetch = None;
        let batch = take_next_batch(&mut range_to_fetch, 5);
        assert_eq!(batch, None);
        assert_eq!(range_to_fetch, None);
    }

    #[test]
    fn take_next_batch_zero_batch() {
        let mut range_to_fetch = Some(1..=5);
        let batch = take_next_batch(&mut range_to_fetch, 0);
        assert_eq!(batch, None);
        assert_eq!(range_to_fetch, Some(1..=5));
    }
}
