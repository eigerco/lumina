use std::collections::HashMap;

use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, warn};
use wasm_bindgen::prelude::*;

use crate::commands::{
    Command, CommandWithResponder, ManagementCommand, NodeCommand, NodeSubscription, WorkerError,
    WorkerResponse, WorkerResult,
};
use crate::error::{Context, Error, Result};
use crate::ports::{request_port, MessageId, MultiplexMessage, Port};
use lumina_utils::executor::{spawn, JoinHandle};

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

    pub async fn subscribe(&self, subscription: NodeSubscription) -> Result<(), WorkerError> {
        let command = Command::Subscribe(subscription);

        let response = self.send(command).await?;
        debug_assert!(matches!(response, WorkerResponse::Ok));

        Ok(())
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
        let (port, incoming_responses) = request_port(port.into())?;

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
                    let MultiplexMessage { id, payload} = msg
                        .ok_or(Error::new("Incoming message channel closed, should not happen"))?;
                    self.handle_incoming_response(id, payload);
                }
                request = self.outgoing_requests.recv() => {
                    let command_with_responder = request
                        .ok_or(Error::new("Outgoing requests channel closed, should not happen"))?;
                    self.handle_outgoing_request(command_with_responder)?;
                }
            }
        }
    }

    fn handle_incoming_response(&mut self, id: MessageId, payload: Option<WorkerResult>) {
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
        CommandWithResponder { command, responder }: CommandWithResponder,
    ) -> Result<()> {
        let mid = self.next_message_index.post_increment();

        if let Err(e) = self.send_message(mid, command) {
            error!("Failed to send request: {e}");
            drop(responder);
            return Ok(());
        }

        if self.pending_responses_map.insert(mid, responder).is_some() {
            return Err(Error::new("collision in message ids, should not happen"));
        }

        Ok(())
    }

    fn send_message(&mut self, id: MessageId, mut command: Command) -> Result<()> {
        if let Some(port) = command.take_port() {
            self.port
                .send_with_transferable(id, &command, port.into())
                .context("failed to send outgoing request with transferable")?;
        } else {
            self.port
                .send(id, &command)
                .context("failed to send outgoing request without transferable")?;
        }
        Ok(())
    }
}
