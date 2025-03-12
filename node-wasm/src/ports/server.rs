use std::marker::PhantomData;

use futures::future::{FutureExt, LocalBoxFuture};
use futures::stream::{FuturesUnordered, StreamExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, warn};
use wasm_bindgen::prelude::*;

use crate::error::{Context, Error, Result};
use crate::ports::common::{MessageId, MultiplexMessage, Port};
use lumina_node::executor::{spawn, JoinHandle};

/// `Server` aggregates multiple existing [`ServerConnection`]s, receiving `Request`s
/// from the connected clients, as well as handles new [`Client`]s connecting.
pub struct Server<Request, Response> {
    connections: Vec<ServerConnection<Request, Response>>,
    requests_tx: mpsc::UnboundedSender<(Request, oneshot::Sender<Response>)>,
    requests_rx: mpsc::UnboundedReceiver<(Request, oneshot::Sender<Response>)>,
    ports_tx: mpsc::UnboundedSender<JsValue>,
    ports_rx: mpsc::UnboundedReceiver<JsValue>,
}

impl<Request, Response> Server<Request, Response>
where
    Request: Serialize + DeserializeOwned + 'static,
    Response: Serialize + 'static,
{
    /// Create a new `Server` without any client connections. See [`get_port_channel`] for adding a
    /// new connection
    pub fn new() -> Self {
        let (requests_tx, requests_rx) = mpsc::unbounded_channel();
        let (ports_tx, ports_rx) = mpsc::unbounded_channel();

        Server {
            connections: vec![],
            requests_tx,
            requests_rx,
            ports_tx,
            ports_rx,
        }
    }

    /// Receive next `Request` coming from one of the connected clients. Should be called
    /// semi-frequently, as it also transparently handles new clients connecting.
    pub async fn recv(&mut self) -> Result<(Request, oneshot::Sender<Response>)> {
        loop {
            select! {
                request = self.requests_rx.recv() => {
                    return Ok(request.expect("request channel should not drop"))
                }
                port = self.ports_rx.recv() => {
                    if let Err(e) = self.add_connection(port.expect("port channel should not drop")) {
                        error!("Failed to add new client connection: {e}");
                    }
                }
            }
        }
    }

    fn add_connection(&mut self, port: JsValue) -> Result<()> {
        let client_connection =
            ServerConnection::start(port, self.requests_tx.clone(), self.ports_tx.clone())?;
        self.connections.push(client_connection);
        Ok(())
    }

    /// Get a channel for adding new ports with client connections.
    pub fn get_port_channel(&self) -> mpsc::UnboundedSender<JsValue> {
        self.ports_tx.clone()
    }
}

/// Represents a one to one connection with a [`Client`] over a js object with port-like semantics.
pub struct ServerConnection<Request, Response> {
    _worker_join_handle: JoinHandle,
    _worker_drop_guard: DropGuard,
    _transferred_types: PhantomData<(Request, Response)>,
}

impl<Request, Response> ServerConnection<Request, Response>
where
    Request: Serialize + DeserializeOwned + 'static,
    Response: Serialize + 'static,
{
    /// Start a new client connection, spinning up background thread for forwarding messages from
    /// the callback
    pub fn start(
        port: JsValue,
        request_tx: mpsc::UnboundedSender<(Request, oneshot::Sender<Response>)>,
        ports_tx: mpsc::UnboundedSender<JsValue>,
    ) -> Result<ServerConnection<Request, Response>> {
        let cancellation_token = CancellationToken::new();
        let mut worker =
            ServerWorker::new(port, request_tx, ports_tx, cancellation_token.child_token())?;

        let _worker_join_handle = spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Serverworker stopped because of a fatal error: {e}");
            }
        });

        Ok(ServerConnection::<Request, Response> {
            _worker_join_handle,
            _worker_drop_guard: cancellation_token.drop_guard(),
            _transferred_types: PhantomData,
        })
    }
}

struct ServerWorker<Request: Serialize, Response> {
    /// Port over which communication takes place
    port: Port,
    /// Queued requests from the onmessage callback
    incoming_requests: mpsc::UnboundedReceiver<MultiplexMessage<Request>>,
    /// Futures waiting for completion to be send as responses
    pending_responses_map: FuturesUnordered<LocalBoxFuture<'static, (MessageId, Option<Response>)>>,
    /// Channel to send requests and response senders over
    request_tx: mpsc::UnboundedSender<(Request, oneshot::Sender<Response>)>,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl<Request, Response> ServerWorker<Request, Response>
where
    Request: Serialize + DeserializeOwned + 'static,
    Response: Serialize + 'static,
{
    fn new(
        port: JsValue,
        request_tx: mpsc::UnboundedSender<(Request, oneshot::Sender<Response>)>,
        port_queue: mpsc::UnboundedSender<JsValue>,
        cancellation_token: CancellationToken,
    ) -> Result<ServerWorker<Request, Response>> {
        let (incoming_requests_tx, incoming_requests) = mpsc::unbounded_channel();

        let port = Port::new_with_channels(port, incoming_requests_tx, Some(port_queue))?;

        Ok(ServerWorker {
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
                    let MultiplexMessage { id, payload } = msg
                        .ok_or(Error::new("Incoming message channel closed, should not happen"))?;

                    match payload {
                        Some(request) =>
                            self.handle_incoming_request(id, request)?,
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

    fn handle_incoming_request(&mut self, id: MessageId, request: Request) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send((request, response_tx))
            .context("forwarding received request failed, no receiver waiting")?;

        let tagged_response = response_rx.map(move |r| (id, r.ok())).boxed_local();

        self.pending_responses_map.push(tagged_response);

        Ok(())
    }

    fn handle_outgoing_response(&mut self, id: MessageId, payload: Option<Response>) -> Result<()> {
        let message = MultiplexMessage { id, payload };

        self.port
            .send(&message)
            .context("failed to send outgoing response ")?;

        Ok(())
    }
}
