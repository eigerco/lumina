use js_sys::{Function, Reflect};
use serde_wasm_bindgen::{from_value, to_value};
use thiserror::Error;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort};
use tokio::select;

use crate::error::{Context, Result};
use crate::worker::commands::{NodeCommand, WorkerResponse};

const REQUEST_MULTIPLEXER_COMMAND_CHANNEL_SIZE: usize = 64;

// Instead of just supporting communicaton with `MessagePort`, allow using any object which
// provides compatible interface
#[wasm_bindgen]
extern "C" {
    pub type MessagePortLike;

    #[wasm_bindgen (catch , method , structural , js_name = postMessage)]
    pub fn post_message(this: &MessagePortLike, message: &JsValue) -> Result<(), JsValue>;

    # [wasm_bindgen (structural , method , setter , js_name = onmessage)]
    pub fn set_onmessage(this: &MessagePortLike, onmessage: Option<&Function>);
    
    /*
    # [wasm_bindgen (structural , method , setter , js_name = onMessage)]
    pub fn set_onmessage2(this: &MessagePortLike, onmessage: Option<&Function>);
    */
}

#[wasm_bindgen]
extern "C" {
    pub type RuntimePortOnMessage;

    #[wasm_bindgen (catch, method, structural, js_name = addListener)]
    pub fn add_listener(this: &RuntimePortOnMessage, listener: Option<&Function>) -> Result<(), JsValue>;
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
        port: MessagePortLike,
        forward_to: mpsc::Sender<(ClientId, Result<NodeCommand, TypedMessagePortError>)>,
    ) -> Self {
        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = forward_to.clone();
            spawn_local(async move {
                let message: Result<NodeCommand, _> =
                    from_value(ev.data()).map_err(TypedMessagePortError::FailedToConvertValue);

                if let Err(e) = response_tx.send((id, message)).await {
                    // TODO: error handling?
                    error!("message forwarding channel closed: {e}");
                }
            })
        };
        let onmessage = Closure::new(onmessage_callback);
        port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        ClientConnection {
            port,
            _onmessage: onmessage,
        }
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
    control_channel: mpsc::Receiver<MessagePortLike>,
    _request_channel_tx: mpsc::Sender<(ClientId, Result<NodeCommand, TypedMessagePortError>)>,
    request_channel_rx: mpsc::Receiver<(ClientId, Result<NodeCommand, TypedMessagePortError>)>,
}

impl RequestServer {
    pub fn new(control_channel: mpsc::Receiver<MessagePortLike>) -> RequestServer {
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
                    // XXX: better?
                    let port = connection.expect("command channel should not close ?");
                    let client_id = ClientId(self.ports.len());
                    info!("Connecting client {client_id:?}");

                    let port = ClientConnection::new(client_id, port, self._request_channel_tx.clone());
                    self.ports.push(port);
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

#[derive(Error, Debug)]
pub enum TypedMessagePortError {
    #[error("Could not convert JsValue: {0}")]
    FailedToConvertValue(serde_wasm_bindgen::Error),

    #[error("Could not convert JsValue: {0}")]
    FailedToConvertValue2(serde_json::Error),
}

pub struct RequestResponse {
    port: MessagePortLike,
    response_channel:
        Mutex<mpsc::Receiver<Result<WorkerResponse, TypedMessagePortError>>>,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl RequestResponse {
    /*
    pub fn from_messageport(port: MessagePort) -> Self {
        let (tx, rx) = mpsc::channel(1);

        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = tx.clone();
            spawn_local(async move {
                web_sys::console::log_1(&JsValue::from("RESPONSE IN CALLBACK:"));
                web_sys::console::log_1(&v);
                let message: Result<WorkerResponse, _> = from_value(ev.data())
                    .map_err(TypedMessagePortError::FailedToConvertValue);

                if let Err(e) = response_tx.send(message).await {
                    error!("message forwarding channel closed: {e}");
                }
            })
        };

        // TODO: Error handling?
        let onmessage = Closure::new(onmessage_callback);

        if let Ok(listeners) = Reflect::get(&port, &JsValue::from("onMessage")) {
            let listeners = RuntimePortOnMessage::from(listeners);
            listeners.add_listener(Some(onmessage.as_ref().unchecked_ref())).expect("not to fail plz");
        } else if Reflect::has(&port, &JsValue::from("onmessage")).expect("") {
            port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        } else {
            panic!(" unknown port !!!")
        }

        web_sys::console::log_1(&port);
        RequestResponse {
            port,
            response_channel: Mutex::new(rx),
            _onmessage: onmessage,
        }

    }
    */
    pub fn new(port: MessagePortLike) -> Self {
        let (tx, rx) = mpsc::channel(1);

        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = tx.clone();
            spawn_local(async move {
                web_sys::console::log_1(&JsValue::from("RESPONSE IN CALLBACK:"));
                web_sys::console::log_1(&ev);
                let message: Result<WorkerResponse, _> = from_value(ev.data())
                    //v.into_serde()
                    .map_err(TypedMessagePortError::FailedToConvertValue);

                if let Err(e) = response_tx.send(message).await {
                    // TODO: error handling?
                    error!("message forwarding channel closed: {e}");
                }
            })
        };

        // TODO: Error handling?
        let onmessage = Closure::new(onmessage_callback);

        if let Ok(listeners) = Reflect::get(&port, &JsValue::from("onMessage")) {
            let listeners = RuntimePortOnMessage::from(listeners);
            listeners.add_listener(Some(onmessage.as_ref().unchecked_ref())).expect("not to fail plz");
        } else if Reflect::has(&port, &JsValue::from("onmessage")).expect("") {
            port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        } else {
            panic!(" unknown port !!!")
        }

        web_sys::console::log_1(&port);
        RequestResponse {
            port,
            response_channel: Mutex::new(rx),
            _onmessage: onmessage,
        }
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
/*
#[derive(Serialize, Copy, Clone)]
struct Nothing;

pub struct TypedMessagePort<S, R, D> {
    port: MessagePortLike,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    _message: PhantomData<(S, D, R)>,
}

pub struct TypedMessage<M, D> {
    message: M,
    data: D,
}

impl<S, R, D> TypedMessagePort<S, R, D>
where
    S: Serialize,
    D: Copy + 'static,
    R: DeserializeOwned + 'static,
{
    pub(crate) fn new(
        port: MessagePortLike,
        forward_to: mpsc::Sender<Result<TypedMessage<R, D>, TypedMessagePortError>>,
        data: D,
    ) -> Self {
        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = forward_to.clone();
            spawn_local(async move {
                let message: Result<TypedMessage<R, D>, _> = match from_value(ev.data()) {
                    Ok(message) => Ok(TypedMessage { message, data }),
                    Err(e) => Err(TypedMessagePortError::FailedToConvertValue(e)),
                };

                if let Err(e) = response_tx.try_send(message) {
                    error!("message forwarding channel busy: {e}");
                }
            })
        };
        let onmessage = Closure::new(onmessage_callback);
        port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        Self {
            port,
            _onmessage: onmessage,
            _message: Default::default(),
        }
    }

    pub fn send(&self, message: &S) -> Result<()> {
        let message_value = to_value(message).context("could not serialise message")?;
        self.port
            .post_message(&message_value)
            .context("could not send command to worker")?;
        Ok(())
    }
}
*/

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::MessageChannel;

    wasm_bindgen_test_configure!(run_in_browser);

    /*
    #[wasm_bindgen_test]
    async fn message_passing() {
        let (tx_client, _rx_client) = mpsc::channel(8);
        let (tx_server, mut rx_server) = mpsc::channel(8);
        let channel = MessageChannel::new().unwrap();
        let port_client = channel.port1();
        let port_server = channel.port2();

        let client =
            TypedMessagePort::<u32, String, _>::new(port_client.into(), tx_client);
        let _server = TypedMessagePort::<String, u32, u32>::new(port_server.into(), tx_server, 99);
        client.send(&1).unwrap();
        let server_received = rx_server
            .recv()
            .await
            .expect("channel not to close")
            .unwrap();

        assert_eq!(server_received.message, 1);
        assert_eq!(server_received.data, 99);
    }
    */

    #[wasm_bindgen_test]
    async fn client_server() {
        let channel0 = MessageChannel::new().unwrap();

        let client0 = RequestResponse::new(channel0.port1().into());

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
