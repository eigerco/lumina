use std::collections::HashMap;

use futures::StreamExt;
use futures::stream::LocalBoxStream;
use lumina_utils::executor::{JoinHandle, spawn};
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::error;
use wasm_bindgen::prelude::*;

use crate::commands::{
    Command, CommandWithResponder, HasMessagePort, NodeCommand, SubscriptionCommand, WorkerCommand,
    WorkerError, WorkerResponse, WorkerResult,
};
use crate::error::{Context, Error, Result};
use crate::ports::{MessageId, MessagePortLike, MultiplexMessage, PortSender, split_port};

/// WorkerClient responsible for sending `Command`s and receiving `WorkerResponse`s to them over a port like
/// object.
pub struct WorkerClient {
    request_tx: mpsc::Sender<CommandWithResponder>,
    _worker_join_handle: JoinHandle,
    _worker_drop_guard: DropGuard,
}

impl WorkerClient {
    /// Create a new `WorkerClient` and start a background thread which forwards responses from the js
    /// callback.
    pub fn new(port: JsValue) -> Result<WorkerClient> {
        let cancellation_token = CancellationToken::new();
        let mut worker = Worker::new(port.into(), cancellation_token.child_token())?;

        let (request_tx, request_rx) = mpsc::channel(16);
        let _worker_join_handle = spawn(async move {
            if let Err(e) = worker.run(request_rx).await {
                error!("WorkerClient worker stopped because of a fatal error: {e}");
            }
        });

        Ok(WorkerClient {
            request_tx,
            _worker_join_handle,
            _worker_drop_guard: cancellation_token.drop_guard(),
        })
    }

    /// Send a `WorkerCommand`, awaiting for a `WorkerResponse`
    pub async fn worker(&self, command: WorkerCommand) -> Result<WorkerResponse, WorkerError> {
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
        subscription: SubscriptionCommand,
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
            .await
            .context("could not forward the request to WorkerClient worker")?;

        rx.await.map_err(|_| WorkerError::EmptyResponse)?
    }
}

struct Worker {
    /// Channel to send multiplexed commands over
    command_sender: PortSender,
    /// Channel to receive multiplexed responses
    response_receiver: LocalBoxStream<'static, Result<MultiplexMessage<WorkerResult>>>,
    /// Map of message ids waiting for response to oneshot channels to send the response over
    pending_responses_map: HashMap<MessageId, oneshot::Sender<WorkerResult>>,
    /// MessageId to be used for the next request
    next_message_index: MessageId,
    /// Cancellation token to stop the worker
    cancellation_token: CancellationToken,
}

impl Worker {
    fn new(port: MessagePortLike, cancellation_token: CancellationToken) -> Result<Worker> {
        let (command_sender, event_receiver) = split_port(port)?;

        let response_receiver = event_receiver
            .map(MultiplexMessage::<WorkerResult>::try_from)
            .boxed_local();

        Ok(Worker {
            command_sender,
            response_receiver,
            next_message_index: Default::default(),
            pending_responses_map: Default::default(),
            cancellation_token,
        })
    }

    async fn run(
        &mut self,
        mut outgoing_requests: mpsc::Receiver<CommandWithResponder>,
    ) -> Result<()> {
        loop {
            select! {
                _ = self.cancellation_token.cancelled() => {
                    return Ok(())
                }
                response = self.response_receiver.next() => {
                    match response.ok_or(Error::new("Incoming response channel closed, should not happen"))? {
                        Ok(MultiplexMessage { id, payload}) =>  {
                            if let Some(response_sender) = self.pending_responses_map.remove(&id) {
                                let _ = response_sender.send(payload);
                            };
                        }
                        Err(e) => error!("error receiving message: {e}"),
                    }
                }
                request = outgoing_requests.recv() => {
                    let command_with_responder = request
                        .ok_or(Error::new("Outgoing requests channel closed, should not happen"))?;
                    self.handle_outgoing_request(command_with_responder)?;
                }
            }
        }
    }

    fn handle_outgoing_request(
        &mut self,
        CommandWithResponder {
            mut command,
            responder,
        }: CommandWithResponder,
    ) -> Result<()> {
        let id = self.next_message_index.post_increment();
        let ports: Vec<_> = command.take_port().into_iter().collect();
        let multiplex_message = MultiplexMessage {
            id,
            payload: command,
        };

        if let Err(e) = self.command_sender.send(&multiplex_message, ports.as_ref()) {
            error!("Failed to send request: {e}");
            return Ok(());
        }

        if self.pending_responses_map.insert(id, responder).is_some() {
            return Err(Error::new("collision in message ids, should not happen"));
        }

        Ok(())
    }
}
