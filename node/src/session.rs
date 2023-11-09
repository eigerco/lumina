use celestia_proto::p2p::pb::HeaderRequest;
use celestia_types::ExtendedHeader;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use crate::executor::spawn;
use crate::header_ex::utils::HeaderRequestExt;
use crate::p2p::{P2pCmd, P2pError};

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
            todo!();
        }

        let (response_tx, response_rx) = mpsc::channel(8);

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

                    // Reschedule the missing sub-range
                    if headers_len < requested_amount {
                        let height = height + headers_len;
                        let amount = requested_amount - headers_len;
                        self.send_request(height, amount).await?;
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
            .flat_map(|headers| headers.into_iter())
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
