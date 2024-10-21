use js_sys::{Array, Function, Reflect};
use serde::Serialize;
use serde_wasm_bindgen::{from_value, to_value, Serializer};
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageEvent, MessagePort};

use crate::commands::{NodeCommand, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::utils::MessageEventExt;

// Instead of supporting communication with just `MessagePort`, allow using any object which
// provides compatible interface, eg. `Worker`
#[wasm_bindgen]
extern "C" {
    pub type MessagePortLike;

    #[wasm_bindgen(catch, method, structural, js_name = postMessage)]
    pub fn post_message(this: &MessagePortLike, message: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, method, structural, js_name = postMessage)]
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

pub(crate) enum ClientMessage {
    Command { id: ClientId, command: NodeCommand },
    AddConnection(JsValue),
}

struct ClientConnection {
    port: MessagePortLike,
    onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl ClientConnection {
    fn new(
        id: ClientId,
        port_like_object: JsValue,
        server_tx: mpsc::UnboundedSender<ClientMessage>,
    ) -> Result<Self> {
        let onmessage = Closure::new(move |ev: MessageEvent| {
            if let Some(port) = ev.get_port() {
                if let Err(e) = server_tx.send(ClientMessage::AddConnection(port)) {
                    error!("port forwarding channel closed, shouldn't happen: {e}");
                }
            }

            let command = match from_value(ev.data()) {
                Ok(msg) => msg,
                Err(e) => {
                    error!("could not deserialise message from {id:?}: {e}");
                    return;
                }
            };

            if let Err(e) = server_tx.send(ClientMessage::Command { id, command }) {
                error!("message forwarding channel closed, shouldn't happen: {e}");
            }
        });

        let port = prepare_message_port(port_like_object, &onmessage)
            .context("failed to setup port for ClientConnection")?;

        Ok(ClientConnection { port, onmessage })
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

impl Drop for ClientConnection {
    fn drop(&mut self) {
        unregister_on_message(&self.port, &self.onmessage);
    }
}

pub struct WorkerServer {
    ports: Vec<ClientConnection>,
    client_tx: mpsc::UnboundedSender<ClientMessage>,
    client_rx: mpsc::UnboundedReceiver<ClientMessage>,
}

impl WorkerServer {
    pub fn new() -> WorkerServer {
        let (client_tx, client_rx) = mpsc::unbounded_channel();

        WorkerServer {
            ports: vec![],
            client_tx,
            client_rx,
        }
    }

    pub async fn recv(&mut self) -> Result<(ClientId, NodeCommand)> {
        loop {
            match self
                .client_rx
                .recv()
                .await
                .expect("all of client connections should never close")
            {
                ClientMessage::Command { id, command } => {
                    return Ok((id, command));
                }
                ClientMessage::AddConnection(port) => {
                    let client_id = ClientId(self.ports.len());
                    info!("Connecting client {client_id:?}");

                    match ClientConnection::new(client_id, port, self.client_tx.clone()) {
                        Ok(port) => self.ports.push(port),
                        Err(e) => error!("Failed to setup ClientConnection: {e}"),
                    }
                }
            }
        }
    }

    pub fn get_control_channel(&self) -> mpsc::UnboundedSender<ClientMessage> {
        self.client_tx.clone()
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
    response_channel:
        Mutex<mpsc::UnboundedReceiver<Result<WorkerResponse, serde_wasm_bindgen::Error>>>,
    onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl WorkerClient {
    pub fn new(object: JsValue) -> Result<Self> {
        let (response_tx, response_rx) = mpsc::unbounded_channel();

        let onmessage = Closure::new(move |ev: MessageEvent| {
            if let Err(e) = response_tx.send(from_value(ev.data())) {
                error!("message forwarding channel closed, should not happen: {e}");
            }
        });

        let port = prepare_message_port(object, &onmessage)
            .context("failed to setup port for WorkerClient")?;

        Ok(WorkerClient {
            port,
            response_channel: Mutex::new(response_rx),
            onmessage,
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
                "invalid response, expected InternalPong got {worker_response:?}"
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

        loop {
            let worker_response = response_channel
                .recv()
                .await
                .expect("response channel should never drop")
                .context("error executing command")?;

            if !worker_response.is_internal_pong() {
                return Ok(worker_response);
            }
        }
    }
}

impl Drop for WorkerClient {
    fn drop(&mut self) {
        unregister_on_message(&self.port, &self.onmessage);
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

fn unregister_on_message(port: &MessagePortLike, callback: &Closure<dyn Fn(MessageEvent)>) {
    let object = port.as_ref();

    if Reflect::has(object, &"onMessage".into()).unwrap_or_default() {
        // `runtime.Port` object. Unregistering callback with `removeListener`.
        let listeners =
            Reflect::get(object, &"onMessage".into()).expect("onMessage existence already checked");

        if let Ok(rm_listener) = Reflect::get(&listeners, &"removeListener".into())
            .and_then(|x| x.dyn_into::<Function>())
        {
            let _ = Reflect::apply(&rm_listener, &listeners, &Array::of1(callback.as_ref()));
        }
    } else if Reflect::has(object, &"onmessage".into()).unwrap_or_default() {
        // `MessagePort` object. Unregistering callback by setting `onmessage` to `null`.
        let _ = Reflect::set(object, &"onmessage".into(), &JsValue::NULL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_futures::spawn_local;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    #[wasm_bindgen_test]
    async fn client_server() {
        let mut server = WorkerServer::new();
        let tx = server.get_control_channel();

        // pre-load response
        spawn_local(async move {
            let channel = MessageChannel::new().unwrap();

            tx.send(ClientMessage::AddConnection(channel.port2().into()))
                .unwrap();

            let client0 = WorkerClient::new(channel.port1().into()).unwrap();
            let response = client0.exec(NodeCommand::IsRunning).await.unwrap();
            assert!(matches!(response, WorkerResponse::IsRunning(true)));
        });

        let (client, command) = server.recv().await.unwrap();
        assert!(matches!(command, NodeCommand::IsRunning));
        server.respond_to(client, WorkerResponse::IsRunning(true));
    }
}
