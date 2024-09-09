use js_sys::{Array, Function, Reflect};
use serde_wasm_bindgen::{from_value, to_value};
use tokio::select;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort};

use crate::error::{Context, Error, Result};
use crate::worker::commands::{NodeCommand, WorkerResponse};

const REQUEST_MULTIPLEXER_COMMAND_CHANNEL_SIZE: usize = 64;

// Instead of just supporting communicaton with just `MessagePort`, allow using any object which
// provides compatible interface
#[wasm_bindgen]
extern "C" {
    pub type MessagePortLike;

    #[wasm_bindgen (catch , method , structural , js_name = postMessage)]
    pub fn post_message(this: &MessagePortLike, message: &JsValue) -> Result<(), JsValue>;
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
        port: JsValue,
        forward_to: mpsc::Sender<(ClientId, Result<NodeCommand, TypedMessagePortError>)>,
    ) -> Result<Self> {
        let onmessage = Closure::new(move |ev: MessageEvent| {
            let response_tx = forward_to.clone();
            spawn_local(async move {
                let message: Result<NodeCommand, _> =
                    from_value(ev.data()).map_err(TypedMessagePortError::FailedToConvertValue);

                if let Err(e) = response_tx.send((id, message)).await {
                    error!("message forwarding channel closed, shouldn't happen: {e}");
                }
            })
        });

        register_on_message_callback(&port, &onmessage)
            .context("failed to setup receiving part of ClientConnection")?;

        Ok(ClientConnection {
            port: port
                .dyn_into()
                .context("failed to setup sending part of ClientConnection")?,
            _onmessage: onmessage,
        })
    }

    fn send(&self, message: &WorkerResponse) -> Result<()> {
        let message_value = to_value(message).context("could not serialise message")?;
        self.port
            .post_message(&message_value)
            .context("could not send command to worker")?;
        Ok(())
    }
}

pub struct RequestServer {
    ports: Vec<ClientConnection>,
    control_channel: mpsc::Receiver<JsValue>,
    _request_channel_tx: mpsc::Sender<(ClientId, Result<NodeCommand, TypedMessagePortError>)>,
    request_channel_rx: mpsc::Receiver<(ClientId, Result<NodeCommand, TypedMessagePortError>)>,
}

impl RequestServer {
    pub fn new(control_channel: mpsc::Receiver<JsValue>) -> RequestServer {
        let (tx, rx) = mpsc::channel(REQUEST_MULTIPLEXER_COMMAND_CHANNEL_SIZE);
        RequestServer {
            ports: vec![],
            control_channel,
            _request_channel_tx: tx,
            request_channel_rx: rx,
        }
    }

    pub async fn recv(&mut self) -> (ClientId, Result<NodeCommand, TypedMessagePortError>) {
        loop {
            select! {
                message = self.request_channel_rx.recv() => {
                    return message.expect("request channel should never close");
                },
                connection = self.control_channel.recv() => {
                    let port = connection.expect("command channel should not close ?");
                    let client_id = ClientId(self.ports.len());
                    info!("Connecting client {client_id:?}");

                        match ClientConnection::new(client_id, port, self._request_channel_tx.clone()) {
                            Ok(port) =>
                    self.ports.push(port),
                    Err(e) => {
                        error!("Failed to setup ClientConnection: {e}");
                    }
                        }
                }
            }
        }
    }

    pub fn respond_to(&self, client: ClientId, response: WorkerResponse) {
        info!("Responding to {client:?}");
        if let Err(e) = self.ports[client.0].send(&response) {
            error!("Failed to send response to client: {e}");
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum TypedMessagePortError {
    #[error("Could not convert JsValue: {0}")]
    FailedToConvertValue(serde_wasm_bindgen::Error),
}

pub struct RequestResponse {
    port: MessagePortLike,
    response_channel: Mutex<mpsc::Receiver<Result<WorkerResponse, TypedMessagePortError>>>,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl RequestResponse {
    pub fn new(port: JsValue) -> Result<Self> {
        let (tx, rx) = mpsc::channel(1);

        let onmessage = Closure::new(move |ev: MessageEvent| {
            let response_tx = tx.clone();
            spawn_local(async move {
                let message: Result<WorkerResponse, _> =
                    from_value(ev.data()).map_err(TypedMessagePortError::FailedToConvertValue);

                if let Err(e) = response_tx.send(message).await {
                    error!("message forwarding channel closed, should not happen: {e}");
                }
            })
        });

        register_on_message_callback(&port, &onmessage)?;

        web_sys::console::log_1(&port);

        Ok(RequestResponse {
            port: port
                .dyn_into()
                .context("failed to setup sending part of RequestResponse")?,
            response_channel: Mutex::new(rx),
            _onmessage: onmessage,
        })
    }

    pub(crate) async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse> {
        let mut response_channel = self.response_channel.lock().await;
        let command_value = to_value(&command).context("could not serialise message")?;

        self.port.post_message(&command_value)?;

        web_sys::console::log_1(&JsValue::from("WAITING FOR RESPONSE"));
        let worker_response = response_channel
            .recv()
            .await
            .expect("response channel should never be dropped")
            .context("error executing command")?;

        Ok(worker_response)
    }
}

// helper to hide slight differences in message passing between runtime.Port used by browser
// extensions and everything else
fn register_on_message_callback(
    object: &JsValue,
    callback: &Closure<dyn Fn(MessageEvent)>,
) -> Result<(), Error> {
    if Reflect::has(object, &JsValue::from("onMessage"))
        .context("failed to reflect onMessage property")?
    {
        // Browser extension runtime.Port has `onMessage` property, on which we should call
        // `addListener` on.
        let listeners = Reflect::get(object, &"onMessage".into())
            .context("could not get `onMessage` property")?;
        // XXX: better error checking?
        let add_listener: Function = Reflect::get(&listeners, &"addListener".into())?.dyn_into()?;
        Reflect::apply(&add_listener, &listeners, &Array::of1(callback.as_ref()))
            .context("could not add listener to object")?;
    } else if Reflect::has(object, &JsValue::from("onmessage"))
        .context("failed to reflect onmessage property")?
    {
        // MessagePort, as well as message passing via Worker instance, requires setting
        // `onmessage` property to callback
        let set_onmessage: Function = Reflect::get(object, &"onmessage".into())?.dyn_into()?;
        Reflect::apply(&set_onmessage, object, &Array::of1(callback.as_ref()))
            .context("could not set onmessage callback")?;
    } else {
        return Err(Error::new("Don't know how to register on message callback"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::MessageChannel;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn client_server() {
        let channel0 = MessageChannel::new().unwrap();

        let client0 = RequestResponse::new(channel0.port1().into()).unwrap();

        let (tx, rx) = mpsc::channel(10);
        tx.send(channel0.port2().into()).await.unwrap();

        // pre-load response
        spawn_local(async move {
            let mut server = RequestServer::new(rx);

            let (client, command) = server.recv().await;
            assert!(matches!(command.unwrap(), NodeCommand::IsRunning));
            server.respond_to(client, WorkerResponse::IsRunning(true));
        });

        let response = client0.exec(NodeCommand::IsRunning).await.unwrap();
        assert!(matches!(response, WorkerResponse::IsRunning(true)));
    }
}
