use futures::future::{FutureExt, LocalBoxFuture};
use futures::stream::{FuturesUnordered, LocalBoxStream, StreamExt};
use lumina_utils::executor::spawn;
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::error;
use wasm_bindgen::prelude::*;

use crate::commands::{Command, CommandWithResponder, HasMessagePort, WorkerError, WorkerResult};
use crate::error::{Context, Error, Result};
use crate::ports::{MessagePortLike, MultiplexMessage, PortSender, split_port};

/// `WorkerServer` aggregates multiple existing [`ServerConnection`]s, receiving `Command`s
/// from the connected clients, as well as handles new [`Client`]s connecting.
pub struct WorkerServer {
    connection_workers_drop_guards: Vec<DropGuard>,
    requests_tx: mpsc::UnboundedSender<CommandWithResponder>,
    requests_rx: mpsc::UnboundedReceiver<CommandWithResponder>,
    ports_tx: mpsc::UnboundedSender<JsValue>,
    ports_rx: mpsc::UnboundedReceiver<JsValue>,
}

impl WorkerServer {
    /// Create a new `WorkerServer` without any client connections. See [`get_port_channel`] for adding a
    /// new connection
    pub fn new() -> Self {
        let (requests_tx, requests_rx) = mpsc::unbounded_channel();
        let (ports_tx, ports_rx) = mpsc::unbounded_channel();

        WorkerServer {
            connection_workers_drop_guards: vec![],
            requests_tx,
            requests_rx,
            ports_tx,
            ports_rx,
        }
    }

    /// Receive next `Command` coming from one of the connected clients. Should be called
    /// semi-frequently, as it also transparently handles new clients connecting.
    pub async fn recv(&mut self) -> Result<CommandWithResponder> {
        loop {
            select! {
                request_event = self.requests_rx.recv() => {
                    return Ok(request_event.expect("WorkerServer internal channel should never close"));
                }
                port = self.ports_rx.recv() => {
                    let port = port.expect("port channel should never drop");

                    if let Err(e) = self.spawn_connection_worker(port.into()) {
                        error!("Failed to add new client connection: {e}");
                    }
                }
            }
        }
    }

    pub fn spawn_connection_worker(&mut self, port: MessagePortLike) -> Result<()> {
        // TODO: connection pruning: https://github.com/eigerco/lumina/issues/434
        let cancellation_token = spawn_connection_worker(port, self.requests_tx.clone())?;
        self.connection_workers_drop_guards
            .push(cancellation_token.drop_guard());

        Ok(())
    }

    /// Get a channel for adding new ports with client connections.
    pub fn get_port_channel(&self) -> mpsc::UnboundedSender<JsValue> {
        self.ports_tx.clone()
    }
}

struct ConnectionWorker {
    /// Channel to receive multiplexed comands
    command_receiver: LocalBoxStream<'static, Result<MultiplexMessage<Command>>>,
    /// Channel to send multiplexed responses over
    response_sender: PortSender,
    /// Futures waiting for completion to be send as responses
    pending_responses_map:
        FuturesUnordered<LocalBoxFuture<'static, MultiplexMessage<WorkerResult>>>,
    /// Channel to send requests and response senders over
    command_forwarding_channel: mpsc::UnboundedSender<CommandWithResponder>,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl ConnectionWorker {
    fn new(
        port: MessagePortLike,
        command_forwarding_channel: mpsc::UnboundedSender<CommandWithResponder>,
        cancellation_token: CancellationToken,
    ) -> Result<Self> {
        let (response_sender, event_receiver) = split_port(port)?;

        let command_receiver = event_receiver
            .map(MultiplexMessage::<Command>::try_from)
            .boxed_local();

        Ok(ConnectionWorker {
            command_receiver,
            response_sender,
            pending_responses_map: Default::default(),
            command_forwarding_channel,
            cancellation_token,
        })
    }

    async fn run(&mut self) -> Result<()> {
        loop {
            select! {
                _ = self.cancellation_token.cancelled() => {
                    return Ok(())
                }
                request = self.command_receiver.next() => {
                    match request.ok_or(Error::new("Incoming message channel closed, should not happen"))? {
                        Ok(multiplexed_command) => {
                            self.handle_incoming_multiplexed_command(multiplexed_command)?;
                        }
                        Err(e) => error!("error receiving command: {e}"),
                    }
                }
                res = self.pending_responses_map.next(), if !self.pending_responses_map.is_empty() => {
                    let response =
                            &mut res
                            .ok_or(Error::new("Responses channel closed, should not happen"))?;
                    let ports: Vec<_> = response.payload.take_port().into_iter().collect();
                    self.response_sender
                        .send(response, ports.as_ref())
                        .with_context(|| format!("failed to send outgoing response for {:?}", response.id))?;
                }
            }
        }
    }

    fn handle_incoming_multiplexed_command(
        &mut self,
        MultiplexMessage { id, payload }: MultiplexMessage<Command>,
    ) -> Result<()> {
        let (responder, response_receiver) = oneshot::channel();

        self.command_forwarding_channel
            .send(CommandWithResponder {
                command: payload,
                responder,
            })
            .context("forwarding received request failed, no receiver waiting")?;

        let tagged_response = response_receiver
            .map(move |r| MultiplexMessage {
                id,
                payload: r
                    .map_err(|_: oneshot::error::RecvError| WorkerError::EmptyResponse)
                    .and_then(|v| v),
            })
            .boxed_local();

        self.pending_responses_map.push(tagged_response);

        Ok(())
    }
}

fn spawn_connection_worker(
    port: MessagePortLike,
    request_tx: mpsc::UnboundedSender<CommandWithResponder>,
) -> Result<CancellationToken> {
    let cancellation_token = CancellationToken::new();
    let mut worker = ConnectionWorker::new(port, request_tx, cancellation_token.child_token())?;

    let _worker_join_handle = spawn(async move {
        if let Err(e) = worker.run().await {
            error!("WorkerServer worker stopped because of a fatal error: {e}");
        }
    });

    Ok(cancellation_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use futures::stream::FuturesOrdered;
    use send_wrapper::SendWrapper;
    use tokio::sync::mpsc;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    use crate::commands::{NodeCommand, WorkerCommand, WorkerError, WorkerResponse};
    use crate::utils::MessageChannelExt;
    use crate::worker_client::WorkerClient;

    #[wasm_bindgen_test]
    async fn smoke_test() {
        let (p0, p1) = MessageChannel::new_ports().unwrap();
        let client = WorkerClient::new(p0.into()).unwrap();
        let (stop_tx, stop_rx) = oneshot::channel();

        spawn(async move {
            let (request_tx, mut request_rx) = mpsc::unbounded_channel();
            let _worker_guard = spawn_connection_worker(p1.into(), request_tx)
                .unwrap()
                .drop_guard();

            let CommandWithResponder { command, responder } =
                request_rx.recv().await.expect("failed to recv");
            assert!(matches!(
                command,
                Command::Management(WorkerCommand::InternalPing)
            ));
            responder.send(Ok(WorkerResponse::InternalPong)).unwrap();
            stop_rx.await.unwrap(); // wait for the test to finish before shutting the server
        });

        let response = client
            .worker_exec(WorkerCommand::InternalPing)
            .await
            .unwrap();

        assert!(matches!(response, WorkerResponse::InternalPong));
        stop_tx.send(()).unwrap();
    }

    #[wasm_bindgen_test]
    async fn response_channel_dropped() {
        let (p0, p1) = MessageChannel::new_ports().unwrap();
        let client = WorkerClient::new(p0.into()).unwrap();
        let (stop_tx, stop_rx) = oneshot::channel();

        spawn(async move {
            let (request_tx, mut request_rx) = mpsc::unbounded_channel();
            let _worker_guard = spawn_connection_worker(p1.into(), request_tx)
                .unwrap()
                .drop_guard();
            let CommandWithResponder { command, responder } =
                request_rx.recv().await.expect("failed to recv");
            assert!(matches!(
                command,
                Command::Management(WorkerCommand::InternalPing)
            ));
            drop(responder);
            stop_rx.await.unwrap(); // wait for the test to finish before shutting the server
        });

        let response = client
            .worker_exec(WorkerCommand::InternalPing)
            .await
            .unwrap_err();

        assert!(matches!(response, WorkerError::EmptyResponse));
        stop_tx.send(()).unwrap();
    }

    #[wasm_bindgen_test]
    async fn multiple_channels() {
        const REQUEST_NUMBER: u64 = 16;

        let (p0, p1) = MessageChannel::new_ports().unwrap();
        let client = Arc::new(SendWrapper::new(WorkerClient::new(p0.into()).unwrap()));
        let (stop_tx, stop_rx) = oneshot::channel();

        spawn(async move {
            let (request_tx, mut request_rx) = mpsc::unbounded_channel();
            let _worker_guard = spawn_connection_worker(p1.into(), request_tx)
                .unwrap()
                .drop_guard();
            for i in 0..=REQUEST_NUMBER {
                let CommandWithResponder { command, responder } = request_rx.recv().await.unwrap();
                let Command::Node(NodeCommand::GetSamplingMetadata { height }) = command else {
                    panic!("invalid command");
                };
                assert_eq!(i, height);
                responder
                    .send(Ok(WorkerResponse::EventsChannelName(format!("R:{height}"))))
                    .unwrap();
            }
            stop_rx.await.unwrap(); // wait for the test to finish before shutting the server
        });

        let mut futs = FuturesOrdered::new();
        for i in 0..=REQUEST_NUMBER {
            let client = client.clone();
            futs.push_back(async move {
                let response = client.node_exec(NodeCommand::GetSamplingMetadata { height: i });
                let response = SendWrapper::new(response).await;
                SendWrapper::new(response)
            });
        }

        for i in 0..=REQUEST_NUMBER {
            let response = futs.next().await.unwrap().take();
            let WorkerResponse::EventsChannelName(name) = response.unwrap() else {
                panic!("invalid response");
            };
            assert_eq!(name, format!("R:{i}"));
        }
        stop_tx.send(()).unwrap();
    }
}
