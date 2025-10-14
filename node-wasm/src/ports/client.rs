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
use crate::ports::common::{MessageId, PayloadWithContext, Port};
use crate::ports::MessagePortLike;
use lumina_utils::executor::{spawn, JoinHandle};

/// Client responsible for sending `Request`s and receiving `Response`s to them over a port like
/// object.
pub struct Client<Request, Response> {
    request_tx:
        mpsc::UnboundedSender<(Request, Option<MessagePortLike>, oneshot::Sender<Response>)>,
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
        let mut worker = Worker::new(port, request_rx, cancellation_token.child_token())?;

        let _worker_join_handle = spawn(async move {
            if let Err(e) = worker.run().await {
                error!("Client worker stopped because of a fatal error: {e}");
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
        transferable: Option<MessagePortLike>,
    ) -> Result<impl Future<Output = Option<Response>>> {
        let (tx, rx) = oneshot::channel();
        self.request_tx
            .send((request, transferable, tx))
            .context("could not forward the request to client worker")?;
        Ok(rx.map(|r| r.ok()))
    }
}

struct Worker<Request, Response: Serialize> {
    /// Port over which communication takes place
    port: Port,
    /// Queued responses from the onmessage callback
    incoming_responses: mpsc::UnboundedReceiver<PayloadWithContext<Response>>,
    /// Map of message ids waiting for response to oneshot channels to send the response over
    pending_responses_map: HashMap<MessageId, oneshot::Sender<Response>>,
    /// Queued requests to be sent
    outgoing_requests:
        mpsc::UnboundedReceiver<(Request, Option<MessagePortLike>, oneshot::Sender<Response>)>,
    /// MessageId to be used for the next request
    next_message_index: MessageId,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl<Request, Response> Worker<Request, Response>
where
    Request: Serialize,
    Response: Serialize + DeserializeOwned + 'static,
{
    fn new(
        port: JsValue,
        request_tx: mpsc::UnboundedReceiver<(
            Request,
            Option<MessagePortLike>,
            oneshot::Sender<Response>,
        )>,
        cancellation_token: CancellationToken,
    ) -> Result<Worker<Request, Response>> {
        let (incoming_responses_tx, incoming_responses) = mpsc::unbounded_channel();

        let port = Port::new_with_channels(port, incoming_responses_tx)?;

        Ok(Worker {
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
                    // we don't expect server sending us ports, ignore
                    let PayloadWithContext { id, payload, .. } = msg
                        .ok_or(Error::new("Incoming message channel closed, should not happen"))?;
                    self.handle_incoming_response(id, payload);
                }
                request = self.outgoing_requests.recv() => {
                    let (msg, port, response_tx) = request
                        .ok_or(Error::new("Outgoing requests channel closed, should not happen"))?;
                    self.handle_outgoing_request(msg, port, response_tx)?;
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
        port: Option<MessagePortLike>,
        response_tx: oneshot::Sender<Response>,
    ) -> Result<()> {
        let mid = self.next_message_index.post_increment();
        /*
                let message = MultiplexMessage {
                    id: mid,
                    payload: Some(request),
                };
        */

        if let Err(e) = self.send_message(mid, request, port) {
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
        id: MessageId,
        payload: Request,
        port: Option<MessagePortLike>,
    ) -> Result<()> {
        if let Some(port) = port {
            self.port
                .send_with_transferable(id, &payload, port.into())
                .context("failed to send outgoing request with transferable")?;
        } else {
            self.port
                .send(id, &payload)
                .context("failed to send outgoing request without transferable")?;
        }
        Ok(())
    }
}
