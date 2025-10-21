use std::collections::HashMap;

use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::error;
use wasm_bindgen::prelude::*;

use crate::commands::{
    Command, CommandWithResponder, ManagementCommand, NodeCommand, NodeSubscription,
    PayloadWithTransferable, WorkerError, WorkerResponse, WorkerResult,
};
use crate::error::{Context, Error, Result};
use crate::ports::{MessageId, MessagePortLike, MultiplexMessage, Port, prepare_port};
use lumina_utils::executor::{JoinHandle, spawn};

/// WorkerClient responsible for sending `Command`s and receiving `WorkerResponse`s to them over a port like
/// object.
pub struct WorkerClient {
    request_tx: mpsc::UnboundedSender<CommandWithResponder>,
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
    pub async fn management(
        &self,
        command: ManagementCommand,
    ) -> Result<WorkerResponse, WorkerError> {
        let command = Command::Management(command);
        self.send(command).await
    }

    /// Send a `NodeCommand`, awaiting for a `WorkerResponse`
    pub async fn node(&self, command: NodeCommand) -> Result<WorkerResponse, WorkerError> {
        let command = Command::Node(command);
        self.send(command).await
    }

    pub async fn subscribe(
        &self,
        subscription: NodeSubscription,
    ) -> Result<MessagePortLike, WorkerError> {
        let command = Command::Subscribe(subscription);

        let port = self
            .send(command)
            .await?
            .into_subscribed()
            .map_err(|_| WorkerError::InvalidResponseType)?;

        port.ok_or(WorkerError::EmptyResponse)
    }

    /// Send a `Command` over the port and return a channel to receive `WorkerResponse` over
    async fn send(&self, command: Command) -> Result<WorkerResponse, WorkerError> {
        let (responder, rx) = oneshot::channel();
        self.request_tx
            .send(CommandWithResponder { command, responder })
            .context("could not forward the request to WorkerClient worker")?;

        rx.await.map_err(|_| WorkerError::EmptyResponse)?
    }
}

struct Worker {
    /// Port over which communication takes place
    port: Port,
    /// Queued responses from the onmessage callback
    incoming_responses: mpsc::UnboundedReceiver<MultiplexMessage<WorkerResult>>,
    /// Map of message ids waiting for response to oneshot channels to send the response over
    pending_responses_map: HashMap<MessageId, oneshot::Sender<WorkerResult>>,
    /// Queued requests to be sent
    outgoing_requests: mpsc::UnboundedReceiver<CommandWithResponder>,
    /// MessageId to be used for the next request
    next_message_index: MessageId,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl Worker {
    fn new(
        port: JsValue,
        request_tx: mpsc::UnboundedReceiver<CommandWithResponder>,
        cancellation_token: CancellationToken,
    ) -> Result<Worker> {
        let (port, incoming_responses) = prepare_port::<WorkerResult>(port.into())?;

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
                    let MultiplexMessage { id, payload} = msg
                        .ok_or(Error::new("Incoming message channel closed, should not happen"))?;
                    if let Some(response_sender) = self.pending_responses_map.remove(&id) {
                        let _ = response_sender.send(payload);
                    };
                }
                request = self.outgoing_requests.recv() => {
                    let command_with_responder = request
                        .ok_or(Error::new("Outgoing requests channel closed, should not happen"))?;
                    self.handle_outgoing_request(command_with_responder)?;
                }
            }
        }
    }

    fn handle_outgoing_request(
        &mut self,
        CommandWithResponder { command, responder }: CommandWithResponder,
    ) -> Result<()> {
        let id = self.next_message_index.post_increment();
        if let Err(e) = self.send_message(MultiplexMessage {
            id,
            payload: command,
        }) {
            error!("Failed to send request: {e}");
            return Ok(());
        }

        if self.pending_responses_map.insert(id, responder).is_some() {
            return Err(Error::new("collision in message ids, should not happen"));
        }

        Ok(())
    }

    fn send_message(&mut self, mut message: MultiplexMessage<Command>) -> Result<()> {
        let port = message.payload.take_transferable();
        if let Some(port) = port {
            self.port
                .send_with_transferable(&message, port.into())
                .context("failed to send outgoing request with transferable")?;
        } else {
            self.port
                .send(&message)
                .context("failed to send outgoing request without transferable")?;
        }
        Ok(())
    }
}
