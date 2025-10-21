use futures::future::{FutureExt, LocalBoxFuture};
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::error;
use wasm_bindgen::prelude::*;

use crate::commands::{Command, CommandWithResponder, WorkerError, WorkerResult};
use crate::error::{Context, Error, Result};
use crate::ports::{prepare_port, MessagePortLike, MultiplexMessage, Port};
use lumina_utils::executor::spawn;

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
    /// Port over which communication takes place
    port: Port,
    /// Queued requests from the onmessage callback
    incoming_requests: mpsc::UnboundedReceiver<MultiplexMessage<Command>>,
    /// Futures waiting for completion to be send as responses
    pending_responses_map:
        FuturesUnordered<LocalBoxFuture<'static, MultiplexMessage<WorkerResult>>>,
    /// Channel to send requests and response senders over
    request_tx: mpsc::UnboundedSender<CommandWithResponder>,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl ConnectionWorker {
    fn new(
        port: MessagePortLike,
        request_tx: mpsc::UnboundedSender<CommandWithResponder>,
        cancellation_token: CancellationToken,
    ) -> Result<ConnectionWorker> {
        let (port, incoming_requests) = prepare_port::<Command>(port)?;

        Ok(ConnectionWorker {
            port,
            incoming_requests,
            pending_responses_map: Default::default(),
            request_tx,
            cancellation_token,
        })
    }

    async fn run(&mut self) -> Result<()> {
        loop {
            select! {
                _ = self.cancellation_token.cancelled() => {
                    return Ok(())
                }
                msg = self.incoming_requests.recv() => {
                    self.handle_incoming_request(
                        msg
                        .ok_or(Error::new("Incoming message channel closed, should not happen"))?
                    )?;
                }
                res = self.pending_responses_map.next(), if !self.pending_responses_map.is_empty() => {
                    self.handle_outgoing_response(
                        &res
                        .ok_or(Error::new("Responses channel closed, should not happen"))?
                    )?;
                }
            }
        }
    }

    fn handle_incoming_request(
        &mut self,
        MultiplexMessage { id, payload }: MultiplexMessage<Command>,
    ) -> Result<()> {
        let (responder, response_receiver) = oneshot::channel();

        self.request_tx
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
                    .flatten(),
            })
            .boxed_local();

        self.pending_responses_map.push(tagged_response);

        Ok(())
    }

    fn handle_outgoing_response(
        &mut self,
        response: &MultiplexMessage<WorkerResult>,
    ) -> Result<()> {
        self.port
            .send(response)
            .with_context(|| format!("failed to send outgoing response for {:?}", response.id))?;

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
    use crate::commands::{ManagementCommand, NodeCommand, WorkerError, WorkerResponse};
    use crate::worker_client::WorkerClient;

    use tokio::sync::mpsc;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    #[wasm_bindgen_test]
    async fn smoke_test() {
        let channel = MessageChannel::new().unwrap();
        let worker_port: JsValue = channel.port2().into();
        let client = WorkerClient::new(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let _worker_guard = spawn_connection_worker(worker_port.into(), request_tx)
            .unwrap()
            .drop_guard();

        let response = client.management(ManagementCommand::InternalPing);

        let CommandWithResponder { command, responder } =
            request_rx.recv().await.expect("failed to recv");
        assert!(matches!(
            command,
            Command::Management(ManagementCommand::InternalPing)
        ));
        responder.send(Ok(WorkerResponse::InternalPong)).unwrap();

        assert!(matches!(
            response.await.unwrap(),
            WorkerResponse::InternalPong
        ));
    }

    #[wasm_bindgen_test]
    async fn response_channel_dropped() {
        let channel = MessageChannel::new().unwrap();
        let worker_port: JsValue = channel.port2().into();
        let client = WorkerClient::new(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let _worker_guard = spawn_connection_worker(worker_port.into(), request_tx)
            .unwrap()
            .drop_guard();

        let response = client.management(ManagementCommand::InternalPing);

        let CommandWithResponder { command, responder } =
            request_rx.recv().await.expect("failed to recv");
        assert!(matches!(
            command,
            Command::Management(ManagementCommand::InternalPing)
        ));
        drop(responder);

        assert!(matches!(
            response.await.unwrap_err(),
            WorkerError::EmptyResponse,
        ));
    }

    #[wasm_bindgen_test]
    async fn multiple_channels() {
        let channel = MessageChannel::new().unwrap();
        let worker_port: JsValue = channel.port2().into();
        let client = WorkerClient::new(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let _worker_guard = spawn_connection_worker(worker_port.into(), request_tx)
            .unwrap()
            .drop_guard();

        let mut responses = vec![];
        for i in 0..=5 {
            responses.push(Some(
                client.node(NodeCommand::GetSamplingMetadata { height: i }),
            ))
        }

        for i in 0..=5 {
            let CommandWithResponder { command, responder } = request_rx.recv().await.unwrap();
            let Command::Node(NodeCommand::GetSamplingMetadata { height }) = command else {
                panic!("invalid command");
            };
            assert_eq!(i, height);
            responder
                .send(Ok(WorkerResponse::EventsChannelName(format!("R:{height}"))))
                .unwrap();
        }

        for i in (0..=5).rev() {
            let WorkerResponse::EventsChannelName(response) =
                responses[i].take().unwrap().await.unwrap()
            else {
                panic!("invalid response");
            };
            assert_eq!(response, format!("R:{i}"));
        }
    }
}
