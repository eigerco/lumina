use std::collections::HashMap;
use std::future::Future;

use futures::future::FutureExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, warn};
use wasm_bindgen::prelude::*;

use crate::error::{Context, Error, Result};
use crate::ports::common::{MessageId, MultiplexMessage, Port, Transferable};
use lumina_node::executor::{spawn, JoinHandle};

/// Client responsible for sending `Request`s and receiving `Response`s to them over a port like
/// object.
pub struct Client<Request, Response> {
    request_tx: mpsc::UnboundedSender<(Request, Transferable, oneshot::Sender<Response>)>,
    _worker_join_handle: JoinHandle,
    _worker_drop_guard: DropGuard,
}

impl<Request, Response> Client<Request, Response>
where
    Request: Serialize + 'static,
    Response: Serialize + DeserializeOwned + 'static,
{
    /// Create a new `Client` and start a background thread which forwards responses from the js
    /// callback.
    pub fn start(port: JsValue) -> Result<Client<Request, Response>> {
        let cancellation_token = CancellationToken::new();
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let mut worker = ClientWorker::new(port, request_rx, cancellation_token.child_token())?;

        let _worker_join_handle = spawn(async move {
            if let Err(e) = worker.run().await {
                error!("ClientWorker stopped because of a fatal error: {e}");
            }
        });

        Ok(Client {
            request_tx,
            _worker_join_handle,
            _worker_drop_guard: cancellation_token.drop_guard(),
        })
    }

    /// Send a `Request` over the port and return a channel to receive `Response` over
    pub fn send(
        &self,
        request: Request,
        transferable: Option<JsValue>,
    ) -> Result<impl Future<Output = Option<Response>>> {
        let (tx, rx) = oneshot::channel();
        self.request_tx
            .send((request, transferable, tx))
            .context("could not forward the request to ClientWorker")?;
        Ok(rx.map(|r| r.ok()))
    }
}

struct ClientWorker<Request, Response: Serialize> {
    /// Port over which communication takes place
    port: Port,
    /// Queued responses from the onmessage callback
    incoming_responses: mpsc::UnboundedReceiver<MultiplexMessage<Response>>,
    /// Map of message ids waiting for response to oneshot channels to send the response over
    pending_responses_map: HashMap<MessageId, oneshot::Sender<Response>>,
    /// Queued requests to be sent
    outgoing_requests: mpsc::UnboundedReceiver<(Request, Transferable, oneshot::Sender<Response>)>,
    /// MessageId to be used for the next request
    next_message_index: MessageId,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl<Request, Response> ClientWorker<Request, Response>
where
    Request: Serialize,
    Response: Serialize + DeserializeOwned + 'static,
{
    fn new(
        port: JsValue,
        request_tx: mpsc::UnboundedReceiver<(Request, Transferable, oneshot::Sender<Response>)>,
        cancellation_token: CancellationToken,
    ) -> Result<ClientWorker<Request, Response>> {
        let (incoming_responses_tx, incoming_responses) = mpsc::unbounded_channel();

        let port = Port::new_with_channels(port, incoming_responses_tx, None)?;

        Ok(ClientWorker {
            port,
            incoming_responses,
            outgoing_requests: request_tx,
            next_message_index: Default::default(),
            pending_responses_map: Default::default(),
            cancellation_token,
        })
    }

    async fn run(&mut self) -> Result<()> {
        loop {
            select! {
                _ = self.cancellation_token.cancelled() => {
                    return Ok(())
                }
                msg = self.incoming_responses.recv() => {
                    let MultiplexMessage { id, payload } = msg
                        .ok_or(Error::new("Incoming message channel closed, should not happen"))?;
                    self.handle_incoming_response(id, payload);
                }
                request = self.outgoing_requests.recv() => {
                    let (msg, transferable, response_tx) = request
                        .ok_or(Error::new("Outgoing requests channel closed, should not happen"))?;
                    self.handle_outgoing_request(msg, transferable, response_tx)?;
                }
            }
        }
    }

    fn handle_incoming_response(&mut self, id: MessageId, payload: Option<Response>) {
        let Some(response_sender) = self.pending_responses_map.remove(&id) else {
            warn!("received unsolicited response for {id:?}, ignoring");
            return;
        };

        // empty response means the responder on the server side was dropped, without
        // sending a response, so let's do the same here
        if let Some(response) = payload {
            let _ = response_sender.send(response);
        }
    }

    fn handle_outgoing_request(
        &mut self,
        request: Request,
        transferable: Transferable,
        response_tx: oneshot::Sender<Response>,
    ) -> Result<()> {
        let mid = self.next_message_index.post_increment();
        let message = MultiplexMessage {
            id: mid,
            payload: Some(request),
        };

        if let Err(e) = self.send_message(message, transferable) {
            error!("Failed to send request: {e}");
            drop(response_tx);
            return Ok(());
        }

        if self
            .pending_responses_map
            .insert(mid, response_tx)
            .is_some()
        {
            return Err(Error::new("collision in message ids, should not happen"));
        }

        Ok(())
    }

    fn send_message(
        &mut self,
        message: MultiplexMessage<Request>,
        transferable: Transferable,
    ) -> Result<()> {
        if let Some(transferable) = transferable {
            self.port
                .send_with_transferable(&message, transferable)
                .context("failed to send outgoing request with transferable")?;
        } else {
            self.port
                .send(&message)
                .context("failed to send outgoing request without transferable")?;
        }
        Ok(())
    }
}
