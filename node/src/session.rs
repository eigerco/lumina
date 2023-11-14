use celestia_proto::p2p::pb::HeaderRequest;
use celestia_types::ExtendedHeader;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use crate::executor::spawn;
use crate::header_ex::utils::HeaderRequestExt;
use crate::p2p::{HeaderExError, P2pCmd, P2pError};

const MAX_AMOUNT_PER_REQ: u64 = 64;
const MAX_CONCURRENT_REQS: usize = 8;

type Result<T, E = P2pError> = std::result::Result<T, E>;

pub(crate) struct Session {
    next_height: u64,
    remaining_amount: u64,
    cmd_tx: mpsc::Sender<P2pCmd>,
    response_tx: mpsc::Sender<(u64, u64, Result<Vec<ExtendedHeader>>)>,
    response_rx: mpsc::Receiver<(u64, u64, Result<Vec<ExtendedHeader>>)>,
    ongoing: usize,
}

impl Session {
    pub(crate) fn new(from_height: u64, amount: u64, cmd_tx: mpsc::Sender<P2pCmd>) -> Result<Self> {
        if from_height < 1 || amount < 1 {
            return Err(P2pError::HeaderEx(HeaderExError::InvalidRequest));
        }

        let (response_tx, response_rx) = mpsc::channel(MAX_CONCURRENT_REQS);

        Ok(Session {
            next_height: from_height,
            remaining_amount: amount,
            cmd_tx,
            response_tx,
            response_rx,
            ongoing: 0,
        })
    }

    pub(crate) async fn run(&mut self) -> Result<Vec<ExtendedHeader>> {
        let mut responses = Vec::new();

        for _ in 0..MAX_CONCURRENT_REQS {
            if self.remaining_amount == 0 {
                break;
            }

            self.send_next_request().await?;
        }

        while self.ongoing > 0 {
            let (height, requested_amount, res) = self.recv_response().await;

            match res {
                Ok(headers) => {
                    let headers_len = headers.len() as u64;

                    responses.push(headers);

                    if headers_len < requested_amount {
                        // Reschedule the missing sub-range
                        let height = height + headers_len;
                        let amount = requested_amount - headers_len;
                        self.send_request(height, amount).await?;
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

        let mut headers = responses
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        headers.sort_unstable_by_key(|header| header.height().value());

        Ok(headers)
    }

    async fn recv_response(&mut self) -> (u64, u64, Result<Vec<ExtendedHeader>>) {
        let (height, requested_amount, res) =
            self.response_rx.recv().await.expect("channel never closes");

        self.ongoing -= 1;

        (height, requested_amount, res)
    }

    pub(crate) async fn send_next_request(&mut self) -> Result<()> {
        if self.remaining_amount == 0 {
            return Ok(());
        }

        let amount = self.remaining_amount.min(MAX_AMOUNT_PER_REQ);
        self.send_request(self.next_height, amount).await?;

        self.next_height += amount;
        self.remaining_amount -= amount;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::spawn;
    use crate::p2p::P2p;
    use crate::store::InMemoryStore;
    use celestia_types::test_utils::ExtendedHeaderGenerator;

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[async_test]
    async fn retry_on_missing_range() {
        let (_p2p, mut p2p_mock) = P2p::<InMemoryStore>::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(64);

        let mut session = Session::new(1, 64, p2p_mock.cmd_tx.clone()).unwrap();
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
        let (_p2p, mut p2p_mock) = P2p::<InMemoryStore>::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(520);

        let mut session = Session::new(1, 520, p2p_mock.cmd_tx.clone()).unwrap();
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
        assert_eq!(headers, received_headers);
    }

    #[async_test]
    async fn not_found_is_not_fatal() {
        let (_p2p, mut p2p_mock) = P2p::<InMemoryStore>::mocked();
        let mut gen = ExtendedHeaderGenerator::new();
        let headers = gen.next_many(64);

        let mut session = Session::new(1, 64, p2p_mock.cmd_tx.clone()).unwrap();
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
        let (_p2p, mut p2p_mock) = P2p::<InMemoryStore>::mocked();

        let mut session = Session::new(1, 64, p2p_mock.cmd_tx.clone()).unwrap();
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
}
