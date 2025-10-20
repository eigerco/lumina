use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde_wasm_bindgen::from_value;
use tokio::select;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, warn};
use wasm_bindgen::prelude::*;
use web_sys::MessageChannel;
use web_sys::MessageEvent;

use crate::commands::CheckableResponseExt;
use crate::commands::ManagementCommand;
use crate::commands::NodeCommand;
use crate::commands::NodeSubscription;
use crate::commands::{Command, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::ports::{MessageId, MessagePortLike, PayloadWithContext, Port};
use lumina_utils::executor::{spawn, JoinHandle};

/// WorkerClient responsible for sending `Command`s and receiving `WorkerResponse`s to them over a port like
/// object.
pub struct WorkerClient {
    request_tx: mpsc::UnboundedSender<(
        Command,
        Option<MessagePortLike>,
        oneshot::Sender<WorkerResponse>,
    )>,
    _worker_join_handle: JoinHandle,
    _worker_drop_guard: DropGuard,
}

impl WorkerClient {
    /// Create a new `WorkerClient` and start a background thread which forwards responses from the js
    /// callback.
    pub fn new(port: JsValue) -> Result<WorkerClient> {
        let cancellation_token = CancellationToken::new();
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let mut worker = Worker::new(port, request_rx, cancellation_token.child_token())?;

        let _worker_join_handle = spawn(async move {
            if let Err(e) = worker.run().await {
                error!("WorkerClient worker stopped because of a fatal error: {e}");
            }
        });

        Ok(WorkerClient {
            request_tx,
            _worker_join_handle,
            _worker_drop_guard: cancellation_token.drop_guard(),
        })
    }

    /// Send a `ManagementCommand`, awaiting for a `WorkerResponse`
    pub async fn management(&self, command: ManagementCommand) -> Result<WorkerResponse> {
        let (port, command) = match command {
            ManagementCommand::ConnectPort(port) => (port, ManagementCommand::ConnectPort(None)),
            other_command => (None, other_command),
        };
        self.send(Command::Management(command), port).await
    }

    /// Send a `NodeCommand`, awaiting for a `WorkerResponse`
    pub async fn node(&self, command: NodeCommand) -> Result<WorkerResponse> {
        let command = Command::Node(command);
        self.send(command, None).await
    }

    pub async fn subscribe<T: DeserializeOwned + 'static>(
        &self,
        subscription: NodeSubscription,
    ) -> Result<(mpsc::UnboundedReceiver<T>, Port)> {
        let (server_port, client_port, subscription_stream) = prepare_subscription_port()?;
        let command = Command::Subscribe(subscription);

        let response = self.send(command, Some(server_port)).await?;
        response.into_subscribed().check_variant()??;
        Ok((subscription_stream, client_port))
    }

    /// Send a `Command` over the port and return a channel to receive `WorkerResponse` over
    async fn send(
        &self,
        request: Command,
        transferable: Option<MessagePortLike>,
    ) -> Result<WorkerResponse> {
        let (tx, rx) = oneshot::channel();
        self.request_tx
            .send((request, transferable, tx))
            .context("could not forward the request to WorkerClient worker")?;

        rx.await
            .context("Response oneshot dropped, should not happen")
    }
}

struct Worker {
    /// Port over which communication takes place
    port: Port,
    /// Queued responses from the onmessage callback
    incoming_responses: mpsc::UnboundedReceiver<PayloadWithContext<WorkerResponse>>,
    /// Map of message ids waiting for response to oneshot channels to send the response over
    pending_responses_map: HashMap<MessageId, oneshot::Sender<WorkerResponse>>,
    /// Queued requests to be sent
    outgoing_requests: mpsc::UnboundedReceiver<(
        Command,
        Option<MessagePortLike>,
        oneshot::Sender<WorkerResponse>,
    )>,
    /// MessageId to be used for the next request
    next_message_index: MessageId,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl Worker {
    fn new(
        port: JsValue,
        request_tx: mpsc::UnboundedReceiver<(
            Command,
            Option<MessagePortLike>,
            oneshot::Sender<WorkerResponse>,
        )>,
        cancellation_token: CancellationToken,
    ) -> Result<Worker> {
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

    fn handle_incoming_response(&mut self, id: MessageId, payload: Option<WorkerResponse>) {
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
        request: Command,
        port: Option<MessagePortLike>,
        response_tx: oneshot::Sender<WorkerResponse>,
    ) -> Result<()> {
        let mid = self.next_message_index.post_increment();

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
        payload: Command,
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

fn prepare_subscription_port<T: DeserializeOwned + 'static>(
) -> Result<(MessagePortLike, Port, mpsc::UnboundedReceiver<T>)> {
    let (tx, rx) = mpsc::unbounded_channel();
    let channel = MessageChannel::new()?;

    let server_port = JsValue::from(channel.port1()).into();

    let client_port = Port::new(
        channel.port2().into(),
        move |ev: MessageEvent| -> Result<()> {
            let item: T = from_value(ev.data()).context("could not deserialize message")?;
            tx.send(item)
                .context("forwarding subscription item failed")?;
            Ok(())
        },
    )?;

    Ok((server_port, client_port, rx))
}
