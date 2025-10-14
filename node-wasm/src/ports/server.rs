use futures::future::{FutureExt, LocalBoxFuture};
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, warn};
use wasm_bindgen::prelude::*;

use crate::commands::{Command, ManagementCommand, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::ports::common::{MessageId, MessagePortLike, PayloadWithContext, Port};
use lumina_utils::executor::spawn;

type Request = Command;
type Response = WorkerResponse;

/// `Server` aggregates multiple existing [`ServerConnection`]s, receiving `Request`s
/// from the connected clients, as well as handles new [`Client`]s connecting.
pub struct Server {
    connection_workers_drop_guards: Vec<DropGuard>,
    requests_tx: mpsc::UnboundedSender<(PayloadWithContext<Request>, oneshot::Sender<Response>)>,
    requests_rx: mpsc::UnboundedReceiver<(PayloadWithContext<Request>, oneshot::Sender<Response>)>,
    ports_tx: mpsc::UnboundedSender<JsValue>,
    ports_rx: mpsc::UnboundedReceiver<JsValue>,
}

impl Server {
    /// Create a new `Server` without any client connections. See [`get_port_channel`] for adding a
    /// new connection
    pub fn new() -> Self {
        let (requests_tx, requests_rx) = mpsc::unbounded_channel();
        let (ports_tx, ports_rx) = mpsc::unbounded_channel();

        Server {
            connection_workers_drop_guards: vec![],
            requests_tx,
            requests_rx,
            ports_tx,
            ports_rx,
        }
    }

    /// Receive next `Request` coming from one of the connected clients. Should be called
    /// semi-frequently, as it also transparently handles new clients connecting.
    pub async fn recv(
        &mut self,
    ) -> Result<(PayloadWithContext<Request>, oneshot::Sender<Response>)> {
        loop {
            select! {
                request_event = self.requests_rx.recv() => {
                    let (request, response_sender) = request_event.expect("request channel should never drop");

                    let Some(command) = &request.payload else {
                        warn!("Request with empty command, ignoring");
                        continue;
                    };

                    match command {
                        Command::Meta(ManagementCommand::ConnectPort) => {
                            let Some(port) = request.port else {
                                warn!("ConnectPort with no port, ignoring");
                                continue;
                            };
                            if let Err(e) = self.spawn_connection_worker(port) {
                                error!("Failed to add new client connection: {e}");
                            }
                        }
                        _ => return Ok((request, response_sender)),
                    }

                }
                port = self.ports_rx.recv() => {
                    let port = port.expect("port channel should never drop");

                    if let Err(e) = self.spawn_connection_worker(port.into()) {
                        error!("Failed to add new client connection: {e}");
                    }
                }
            }
            /*
                        let (request, response_sender) = self
                            .requests_rx
                            .recv()
                            .await
                            .expect("request channel should never drop");
                        let Some(command) = &request.payload else {
                            warn!("Request with empty command, ignoring");
                            continue;
                        };
                        tracing::info!("received {command:?}");
                        match command {
                            Command::Meta(ManagementCommand::ConnectPort) => {
                                let Some(port) = request.port else {
                                    warn!("ConnectPort with no port, ignoring");
                                    continue;
                                };
                                if let Err(e) = self.spawn_connection_worker(port) {
                                    error!("Failed to add new client connection: {e}");
                                }
                            }
                            _ => return Ok((request, response_sender)),
                        }
            */
        }
        // loop {
        //     select! {
        //         request = self.requests_rx.recv() => {
        //             return Ok(request.expect("request channel should not drop"))
        //         }
        //         port = self.ports_rx.recv() => {
        //             if let Err(e) = self.spawn_connection_worker(port.expect("port channel should not drop")) {
        //                 error!("Failed to add new client connection: {e}");
        //             }
        //         }
        //     }
        // }
    }

    fn spawn_connection_worker(&mut self, port: MessagePortLike) -> Result<()> {
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
    incoming_requests: mpsc::UnboundedReceiver<PayloadWithContext<Request>>,
    /// Futures waiting for completion to be send as responses
    pending_responses_map: FuturesUnordered<LocalBoxFuture<'static, (MessageId, Option<Response>)>>,
    /// Channel to send requests and response senders over
    request_tx: mpsc::UnboundedSender<(PayloadWithContext<Request>, oneshot::Sender<Response>)>,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl ConnectionWorker
//where
//    Request: Serialize + DeserializeOwned + 'static,
//    Response: Serialize + 'static,
{
    fn new(
        port: MessagePortLike,
        request_tx: mpsc::UnboundedSender<(PayloadWithContext<Request>, oneshot::Sender<Response>)>,
        //port_queue: mpsc::UnboundedSender<JsValue>,
        cancellation_token: CancellationToken,
    ) -> Result<ConnectionWorker> {
        let (incoming_requests_tx, incoming_requests) = mpsc::unbounded_channel();

        let port = Port::new_with_channels(port.into(), incoming_requests_tx)?;

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
                    let PayloadWithContext { id, payload, port } = msg
                        .ok_or(Error::new("Incoming message channel closed, should not happen"))?;

                    match payload {
                        Some(request) =>
                            self.handle_incoming_request(id, request, port)?,
                        None => {
                            warn!("Received message with empty payload");
                            // send back empty message back, in case somebody's waiting
                            self.handle_outgoing_response(id, None)?;
                        }
                    }
                }
                res = self.pending_responses_map.next(), if !self.pending_responses_map.is_empty() => {
                    let (id, response) = res
                        .ok_or(Error::new("Responses channel closed, should not happen"))?;
                    self.handle_outgoing_response(id, response)?;
                }
            }
        }
    }

    fn handle_incoming_request(
        &mut self,
        id: MessageId,
        payload: Request,
        port: Option<MessagePortLike>,
    ) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send((
                PayloadWithContext {
                    id,
                    payload: Some(payload),
                    port,
                },
                response_tx,
            ))
            .context("forwarding received request failed, no receiver waiting")?;

        let tagged_response = response_rx.map(move |r| (id, r.ok())).boxed_local();

        self.pending_responses_map.push(tagged_response);

        Ok(())
    }

    fn handle_outgoing_response(&mut self, id: MessageId, payload: Option<Response>) -> Result<()> {
        self.port
            .send(id, &payload)
            .context("failed to send outgoing response")?;

        Ok(())
    }
}

fn spawn_connection_worker(
    port: MessagePortLike,
    request_tx: mpsc::UnboundedSender<(PayloadWithContext<Request>, oneshot::Sender<Response>)>,
) -> Result<CancellationToken>
//where
    // Request: Serialize + DeserializeOwned + 'static,
    // Response: Serialize + 'static,
{
    let cancellation_token = CancellationToken::new();
    let mut worker = ConnectionWorker::new(port, request_tx, cancellation_token.child_token())?;

    let _worker_join_handle = spawn(async move {
        if let Err(e) = worker.run().await {
            error!("Server worker stopped because of a fatal error: {e}");
        }
    });
    Ok(cancellation_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::client::Client;

    use tokio::sync::mpsc;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    #[wasm_bindgen_test]
    async fn smoke_test() {
        let channel = MessageChannel::new().unwrap();
        let client = Client::<i32, i32>::start(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let (port_tx, _) = mpsc::unbounded_channel();
        let _worker_guard =
            spawn_connection_worker::<i32, i32>(channel.port2().into(), request_tx, port_tx)
                .unwrap()
                .drop_guard();

        let response = client.send(42, None).unwrap();

        let (request, responder) = request_rx.recv().await.expect("failed to recv");
        assert_eq!(request, 42);
        responder.send(43).unwrap();

        assert_eq!(response.await.unwrap(), 43);
    }

    #[wasm_bindgen_test]
    async fn response_channel_dropped() {
        let channel = MessageChannel::new().unwrap();
        let client = Client::<i32, i32>::start(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let (port_tx, _) = mpsc::unbounded_channel();
        let _worker_guard =
            spawn_connection_worker::<i32, i32>(channel.port2().into(), request_tx, port_tx)
                .unwrap()
                .drop_guard();

        let response = client.send(42, None).unwrap();

        let (request, responder) = request_rx.recv().await.expect("failed to recv");
        assert_eq!(request, 42);
        drop(responder);

        assert_eq!(response.await, None);
    }

    #[wasm_bindgen_test]
    async fn multiple_channels() {
        let channel = MessageChannel::new().unwrap();
        let client = Client::<i32, String>::start(channel.port1().into()).unwrap();

        let (request_tx, mut request_rx) = mpsc::unbounded_channel();
        let (port_tx, _) = mpsc::unbounded_channel();
        let _worker_guard =
            spawn_connection_worker::<i32, String>(channel.port2().into(), request_tx, port_tx)
                .unwrap()
                .drop_guard();

        let mut responses = [
            Some(client.send(0, None).unwrap()),
            Some(client.send(1, None).unwrap()),
            Some(client.send(2, None).unwrap()),
            Some(client.send(3, None).unwrap()),
            Some(client.send(4, None).unwrap()),
            Some(client.send(5, None).unwrap()),
        ];

        for i in 0..=5 {
            let (request, responder) = request_rx.recv().await.unwrap();
            assert_eq!(i, request);
            responder.send(format!("R:{request}")).unwrap();
        }

        for i in (0..=5).rev() {
            assert_eq!(
                responses[i].take().unwrap().await.unwrap(),
                format!("R:{i}")
            );
        }
    }
}
