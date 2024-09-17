use js_sys::{Array, Function, Reflect};
use serde::Serialize;
use serde_wasm_bindgen::{from_value, to_value, Serializer};
use tokio::select;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageEvent, MessagePort};

use crate::commands::{NodeCommand, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::utils::{MessageError, MessageEventExt};

// Instead of just supporting communicaton with just `MessagePort`, allow using any object which
// provides compatible interface
#[wasm_bindgen]
extern "C" {
    pub type MessagePortLike;

    #[wasm_bindgen (catch , method , structural , js_name = postMessage)]
    pub fn post_message(this: &MessagePortLike, message: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen (catch , method , structural , js_name = postMessage)]
    pub fn post_message_with_transferable(
        this: &MessagePortLike,
        message: &JsValue,
        transferable: &JsValue,
    ) -> Result<(), JsValue>;
}

impl From<MessagePort> for MessagePortLike {
    fn from(port: MessagePort) -> Self {
        JsValue::from(port).into()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ClientId(usize);

struct ClientConnection {
    port: MessagePortLike,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl ClientConnection {
    fn new(
        id: ClientId,
        object: JsValue,
        forward_messages_to: mpsc::UnboundedSender<(ClientId, Result<NodeCommand, MessageError>)>,
        forward_connects_to: mpsc::UnboundedSender<JsValue>,
    ) -> Result<Self> {
        let onmessage = Closure::new(move |ev: MessageEvent| {
            let message_tx = forward_messages_to.clone();
            let port_tx = forward_connects_to.clone();

            let message = ev.get_message();

            if let Some(port) = ev.get_port() {
                if let Err(e) = port_tx.send(port) {
                    error!("port forwarding channel closed, shouldn't happen: {e}");
                }
            }

            if let Err(e) = message_tx.send((id, message)) {
                error!("message forwarding channel closed, shouldn't happen: {e}");
            }
        });

        let port = prepare_message_port(object, &onmessage)
            .context("failed to setup port for ClientConnection")?;

        Ok(ClientConnection {
            port,
            _onmessage: onmessage,
        })
    }

    fn send(&self, message: &WorkerResponse) -> Result<()> {
        let serializer = Serializer::json_compatible();
        let message_value = message
            .serialize(&serializer)
            .context("could not serialise message")?;
        self.port
            .post_message(&message_value)
            .context("could not send command to worker")?;
        Ok(())
    }
}

pub struct WorkerServer {
    ports: Vec<ClientConnection>,
    connect_tx: mpsc::UnboundedSender<JsValue>,
    connect_rx: mpsc::UnboundedReceiver<JsValue>,
    request_tx: mpsc::UnboundedSender<(ClientId, Result<NodeCommand, MessageError>)>,
    request_rx: mpsc::UnboundedReceiver<(ClientId, Result<NodeCommand, MessageError>)>,
}

impl WorkerServer {
    pub fn new() -> WorkerServer {
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let (connect_tx, connect_rx) = mpsc::unbounded_channel();

        WorkerServer {
            ports: vec![],
            connect_tx,
            connect_rx,
            request_tx,
            request_rx,
        }
    }

    pub async fn recv(&mut self) -> Result<(ClientId, NodeCommand)> {
        loop {
            select! {
                message = self.request_rx.recv() => {
                    let (client_id, result) =  message.expect("request channel should never close");
                    let message = result
                        .with_context(|| format!("could not parse command received from {client_id:?}"))?;
                    return Ok((client_id, message));
                },
                connection = self.connect_rx.recv() => {
                    let port = connection.expect("command channel should not close ?");
                    let client_id = ClientId(self.ports.len());
                    info!("Connecting client {client_id:?}");

                        match ClientConnection::new(client_id, port, self.request_tx.clone(), self.connect_tx.clone()) {
                            Ok(port) => self.ports.push(port),
                            Err(e) => error!("Failed to setup ClientConnection: {e}"),
                        }
                }
            }
        }
    }

    pub fn get_connect_channel(&self) -> mpsc::UnboundedSender<JsValue> {
        self.connect_tx.clone()
    }

    pub fn respond_to(&self, client: ClientId, response: WorkerResponse) {
        trace!("Responding to {client:?}");
        if let Err(e) = self.ports[client.0].send(&response) {
            error!("Failed to send response to client: {e}");
        }
    }
}

pub struct WorkerClient {
    port: MessagePortLike,
    response_channel: Mutex<mpsc::UnboundedReceiver<Result<WorkerResponse, MessageError>>>,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl WorkerClient {
    pub fn new(object: JsValue) -> Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();

        let onmessage = Closure::new(move |ev: MessageEvent| {
            let response_tx = tx.clone();
            let message = ev.get_message();

            if let Err(e) = response_tx.send(message) {
                error!("message forwarding channel closed, should not happen: {e}");
            }
        });

        let port = prepare_message_port(object, &onmessage)
            .context("failed to setup port for WorkerClient")?;

        Ok(WorkerClient {
            port,
            response_channel: Mutex::new(rx),
            _onmessage: onmessage,
        })
    }

    pub(crate) async fn add_connection_to_worker(&self, port: &JsValue) -> Result<()> {
        let mut response_channel = self.response_channel.lock().await;

        let command_value =
            to_value(&NodeCommand::InternalPing).context("could not serialise message")?;

        self.port
            .post_message_with_transferable(&command_value, &Array::of1(port))
            .context("could not transfer port")?;

        let worker_response = response_channel
            .recv()
            .await
            .expect("response channel should never drop")
            .context("error adding connection")?;

        if !worker_response.is_internal_pong() {
            Err(Error::new(&format!(
                "invalid response, expected InternalPing got {worker_response:?}"
            )))
        } else {
            Ok(())
        }
    }

    pub(crate) async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse> {
        let mut response_channel = self.response_channel.lock().await;
        let command_value = to_value(&command).context("could not serialise message")?;

        self.port
            .post_message(&command_value)
            .context("could not post message")?;

        let worker_response = response_channel
            .recv()
            .await
            .expect("response channel should never drop")
            .context("error executing command")?;

        Ok(worker_response)
    }
}

// helper to hide slight differences in message passing between runtime.Port used by browser
// extensions and everything else
fn prepare_message_port(
    object: JsValue,
    callback: &Closure<dyn Fn(MessageEvent)>,
) -> Result<MessagePortLike, Error> {
    // check whether provided object has `postMessage` method
    let _post_message: Function = Reflect::get(&object, &"postMessage".into())?
        .dyn_into()
        .context("could not get object's postMessage")?;

    if Reflect::has(&object, &JsValue::from("onMessage"))
        .context("failed to reflect onMessage property")?
    {
        // Browser extension runtime.Port has `onMessage` property, on which we should call
        // `addListener` on.
        let listeners = Reflect::get(&object, &"onMessage".into())
            .context("could not get `onMessage` property")?;

        let add_listener: Function = Reflect::get(&listeners, &"addListener".into())
            .context("could not get `onMessage.addListener` property")?
            .dyn_into()
            .context("expected `onMessage.addListener` to be a function")?;
        Reflect::apply(&add_listener, &listeners, &Array::of1(callback.as_ref()))
            .context("error calling `onMessage.addListener`")?;
    } else if Reflect::has(&object, &JsValue::from("onmessage"))
        .context("failed to reflect onmessage property")?
    {
        // MessagePort, as well as message passing via Worker instance, requires setting
        // `onmessage` property to callback
        Reflect::set(&object, &"onmessage".into(), callback.as_ref())
            .context("could not set onmessage callback")?;
    } else {
        return Err(Error::new("Don't know how to register onmessage callback"));
    }

    Ok(MessagePortLike::from(object))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_futures::spawn_local;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::MessageChannel;

    #[wasm_bindgen_test]
    async fn client_server() {
        let mut server = WorkerServer::new();
        let tx = server.get_connect_channel();

        // pre-load response
        spawn_local(async move {
            let channel = MessageChannel::new().unwrap();

            tx.send(channel.port2().into()).unwrap();

            let client0 = WorkerClient::new(channel.port1().into()).unwrap();

            let response = client0.exec(NodeCommand::IsRunning).await.unwrap();
            assert!(matches!(response, WorkerResponse::IsRunning(true)));
        });

        let (client, command) = server.recv().await;
        assert!(matches!(command.unwrap(), NodeCommand::IsRunning));
        server.respond_to(client, WorkerResponse::IsRunning(true));
    }
}
