use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, Mutex};
use tracing::error;
use wasm_bindgen::closure::Closure;
use thiserror::Error;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort};

use crate::worker::commands::{NodeCommand, WorkerResponse};
use crate::error::{Context, Result};

const REQUEST_MULTIPLEXER_COMMAND_CHANNEL_SIZE: usize = 64;

#[derive(Debug, Clone, Copy)]
pub struct ClientId(usize);

pub struct RequestServer{
    ports: Vec<TypedMessagePort<WorkerResponse, NodeCommand, ClientId>>,
    _command_channel_tx: mpsc::Sender<Result<TypedMessage<NodeCommand, ClientId>, TypedMessagePortError>>,
    command_channel_rx: mpsc::Receiver<Result<TypedMessage<NodeCommand, ClientId>, TypedMessagePortError>>,
}

impl RequestServer {
    pub fn new() -> RequestServer {
        let (tx, rx)= mpsc::channel(REQUEST_MULTIPLEXER_COMMAND_CHANNEL_SIZE);
            RequestServer {
            ports: vec![],
            _command_channel_tx: tx,
            command_channel_rx: rx,
        }
    }

    pub fn connect(&mut self, port: MessagePort) {
        let client_id = ClientId(self.ports.len());

        let port = TypedMessagePort::new(port, self._command_channel_tx.clone(), client_id);

        self.ports.push(port);
    }

    pub async fn recv(&mut self) -> Result<(NodeCommand, ClientId)> {
        let TypedMessage { message, data } = self.command_channel_rx.recv().await.expect("command channel should never close")?;
        Ok((message, data))
    }

    pub fn respond_to(&self, client: ClientId, response: WorkerResponse) {
        if let Err(e) =  self.ports[client.0].send(&response) {
            error!("Failed to send response to client: {e}");
        }
    }

}

#[derive(Error, Debug)]
pub enum TypedMessagePortError {
    #[error("Could not convert JsValue: {0}")]
    FailedToConvertValue(serde_wasm_bindgen::Error)
}

#[derive(Serialize, Copy, Clone)]
struct Nothing;

pub struct RequestResponse {
    port: TypedMessagePort<NodeCommand, WorkerResponse, Nothing>,
    response_channel: Mutex<mpsc::Receiver<Result<TypedMessage<WorkerResponse, Nothing>, TypedMessagePortError>>>,
}

impl RequestResponse {
    pub fn new(port: MessagePort) -> Self {
        let (tx, rx) = mpsc::channel(1);

        RequestResponse {
            port: TypedMessagePort::new(port, tx, Nothing),
            response_channel: Mutex::new(rx)
        }
    }

    pub(crate) async fn exec(&self, command: NodeCommand) -> Result<WorkerResponse> {
        let mut response_channel = self.response_channel.lock().await;

        self.port.send(&command)?;
        println!("senttt");

        let TypedMessage { message, ..} = response_channel.recv().await
            .expect("response channel should never be dropped")
            .context("error executing command")?;

        Ok(message)
    }
}

pub struct TypedMessagePort<S, R, D>{ 
    port: MessagePort,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    _message: PhantomData<(S, D, R)>,
}

pub struct TypedMessage<M, D> {
    message: M,
    data: D
}

impl<S, R, D> TypedMessagePort<S, R, D> 
where S: Serialize,
      D: Copy + 'static,
      R: DeserializeOwned + 'static
{
    pub(crate) fn new(port: MessagePort, forward_to: mpsc::Sender<Result<TypedMessage<R, D>, TypedMessagePortError>>, data: D) -> Self {
        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = forward_to.clone();
            spawn_local(async move {
                let message: Result<TypedMessage<R, D>, _> = match from_value(ev.data()) {
                    Ok(message) => Ok(
                        TypedMessage {
                            message,
                            data
                        }
                    ),
                    Err(e) => {
                        Err(TypedMessagePortError::FailedToConvertValue(e))
                    }
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
        self.port.post_message(&message_value).context("could not send command to worker")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use web_sys::MessageChannel;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn message_passing() {
        let (tx_client, _rx_client) = mpsc::channel(8);
        let (tx_server, mut rx_server) = mpsc::channel(8);
        let channel = MessageChannel::new().unwrap();
        let port_client = channel.port1();
        let port_server = channel.port2();

        let client = TypedMessagePort::<u32, String, _>::new(port_client, tx_client, Nothing);
        let _server = TypedMessagePort::<String, u32, u32>::new(port_server, tx_server, 99);
        client.send(&1).unwrap();
        let server_received = rx_server.recv().await.expect("channel not to close").unwrap();

        assert_eq!(server_received.message, 1);
        assert_eq!(server_received.data, 99);
    }

    #[wasm_bindgen_test]
    async fn client_server() {
        let channel0 = MessageChannel::new().unwrap();
        let channel1 = MessageChannel::new().unwrap();

        let client0 = RequestResponse::new(channel0.port1());
        let _client1 = RequestResponse::new(channel1.port1());

        // pre-load response 
        spawn_local(async move {
            let mut server = RequestServer::new();
            server.connect(channel0.port2());
            server.connect(channel1.port2());

            let (_command, client) = server.recv().await.unwrap();
            server.respond_to(client, WorkerResponse::IsRunning(true));
        });

        let response = client0.exec(NodeCommand::IsRunning).await.unwrap();
        assert!(matches!(response, WorkerResponse::IsRunning(true)));
    }
}
