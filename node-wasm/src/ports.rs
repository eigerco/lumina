use js_sys::{Array, Function, Reflect};
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::mpsc;
use tracing::{error, info};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageEvent, MessagePort};

use crate::commands::{NodeCommand, WorkerResponse};
use crate::error::{Context, Error, Result};
use crate::oneshot_channel::{self, OneshotSender};
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

impl MessagePortLike {
    fn new(object: JsValue) -> Result<MessagePortLike> {
        let _post_message: Function = Reflect::get(&object, &"postMessage".into())?
            .dyn_into()
            .context("could not get object's postMessage")?;
        Ok(MessagePortLike::from(object))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ClientId(usize);

#[allow(clippy::large_enum_variant)]
pub(crate) enum ClientMessage {
    Command {
        response_sender: OneshotSender<WorkerResponse>,
        command: NodeCommand,
    },
    AddConnection(JsValue),
}

struct ClientConnection {
    port: MessagePortLike,
    onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl ClientConnection {
    fn new(
        cid: ClientId,
        port_like_object: JsValue,
        server_tx: mpsc::UnboundedSender<ClientMessage>,
    ) -> Result<Self> {
        let onmessage = Closure::new(error_logging_onmessage(
            cid,
            move |ev: MessageEvent| -> Result<()> {
                let (response_sender, maybe_command_channel) = ev.get_command_ports()?;
                if let Some(connection_port) = maybe_command_channel {
                    server_tx
                        .send(ClientMessage::AddConnection(connection_port))
                        .context("adding client connection failed")?;
                }

                let command = from_value(ev.data()).context("could not deserialize command")?;

                server_tx
                    .send(ClientMessage::Command {
                        response_sender,
                        command,
                    })
                    .context("forwarding command to worker failed, should not happen")?;
                Ok(())
            },
        ));

        let port = prepare_message_port(port_like_object, &onmessage)
            .context("failed to setup port for ClientConnection")?;

        Ok(ClientConnection { port, onmessage })
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

    pub async fn recv(&mut self) -> Result<(NodeCommand, OneshotSender<WorkerResponse>)> {
        loop {
            match self
                .client_rx
                .recv()
                .await
                .expect("all of client connections should never close")
            {
                ClientMessage::Command {
                    response_sender,
                    command,
                } => {
                    return Ok((command, response_sender));
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
}

pub struct WorkerClient {
    port: MessagePortLike,
}

impl WorkerClient {
    pub fn new(object: JsValue) -> Result<Self> {
        Ok(WorkerClient {
            port: MessagePortLike::new(object)?,
        })
    }

    pub(crate) async fn add_connection_to_worker(&self, port: &JsValue) -> Result<()> {
        let (response_sender, response_receiver) = oneshot_channel::new()?;

        let command_value =
            to_value(&NodeCommand::InternalPing).context("could not serialise message")?;

        self.port
            .post_message_with_transferable(
                &command_value,
                &Array::of2(&response_sender.into_inner(), port),
            )
            .context("could not transfer port")?;

        response_receiver.recv().await
    }

    pub(crate) async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse> {
        let (response_sender, response_receiver) = oneshot_channel::new()?;

        let command_value = to_value(&command).context("could not serialise message")?;

        self.port
            .post_message_with_transferable(
                &command_value,
                &Array::of1(&response_sender.into_inner()),
            )
            .context("could not post message")?;

        response_receiver.recv().await
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

fn error_logging_onmessage<F>(cid: ClientId, f: F) -> impl Fn(MessageEvent)
where
    F: Fn(MessageEvent) -> Result<()>,
{
    move |ev: MessageEvent| {
        if let Err(e) = f(ev) {
            error!("error in onmessage for client {cid:?}: {e}");
        }
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

        let (command, response_sender) = server.recv().await.unwrap();
        assert!(matches!(command, NodeCommand::IsRunning));
        response_sender.send(WorkerResponse::IsRunning(true));
    }
}
