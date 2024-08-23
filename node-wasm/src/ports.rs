use std::marker::PhantomData;

use futures::channel::oneshot;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::{mpsc, Mutex};
use tracing::error;
use wasm_bindgen::closure::Closure;
use thiserror::Error;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort};

use crate::worker::commands::{NodeCommand, WorkerResponse};
use crate::error::{Context, Error, Result};

const REQUEST_MULTIPLEXER_COMMAND_CHANNEL_SIZE: usize = 64;

#[derive(Error, Debug)]
enum RequestMultiplexerError {
    #[error("Busy responding to previous request")]
    ChannelBusy
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ClientId(usize);

struct WorkerMessage{
    command: NodeCommand,
    client: ClientId
}

pub struct RequestMultiplexer{
    ports: Vec<(MessagePort, Closure<dyn Fn(MessageEvent)>)>,
    //callbacks: Vec<Closure<()>>,
    command_channel: (mpsc::Sender<WorkerMessage>, mpsc::Receiver<WorkerMessage>),
}

impl RequestMultiplexer {
    pub fn new() -> RequestMultiplexer {
        RequestMultiplexer {
            ports: vec![],
            command_channel: mpsc::channel(REQUEST_MULTIPLEXER_COMMAND_CHANNEL_SIZE),
        }
    }

    pub fn connect(&mut self, port: MessagePort) {
        let client_id = ClientId(self.ports.len());

        let onmessage_callback =
            get_client_message_callback(self.command_channel.0.clone(), client_id);

        port.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));

        self.ports.push((port, onmessage_callback));
    }

    pub async fn recv(&mut self) -> Result<(NodeCommand, ClientId)> {
        let WorkerMessage { command, client } = self.command_channel.1.recv().await.expect("command channel should never close");
        Ok((command, client))
    }

    pub async fn send(&self, client: ClientId, response: &WorkerResponse) -> Result<()> {
        let response_value = to_value(response).unwrap_or(JsValue::UNDEFINED);

        Ok(self.ports[client.0].0.post_message(&response_value)?)
    }

}

fn get_client_message_callback(
    command_channel: mpsc::Sender<WorkerMessage>,
    client: ClientId,
) -> Closure<dyn Fn(MessageEvent)> {
    Closure::new(move |event: MessageEvent| {
        let command_channel = command_channel.clone();
        spawn_local(async move {
            let Ok(command) = from_value(event.data()) else {
                error!("Could not deserialise command");
                return;
            };

            if let Err(e) = command_channel.send(WorkerMessage {
                command,
                client,
            }).await {
                error!("command channel inside worker closed, should not happen: {e}");
            }
        })
    })
}


pub struct RequestResponse<T, R>{ 
    port: MessagePort,
    response: mpsc::Receiver<Result<R>>,
    _onmessage: Closure<dyn Fn(MessageEvent)>,
    _message: PhantomData<(T, R)>,
}

impl<T, R> RequestResponse<T, R> 
where T: Serialize,
      R: DeserializeOwned + 'static
{
    //pub(crate) fn new_with_callback(port: MessagePort, callback: Closure<MessageEvent>) -> Self { }
    pub(crate) fn new(port: MessagePort) -> Self {
        // single message, act as a rendezvous
        let (tx, rx) = mpsc::channel(1);

        let onmessage_callback = move |ev: MessageEvent| {
            let response_tx = tx.clone();
            spawn_local(async move {
                /*
                let data: Result<R, Error> = match from_value(ev.data()) {
                    Ok(jsvalue) => jsvalue,
                    Err(e) => {
                        error!("WorkerClient could not convert from JsValue: {e}");
                        let error = Error::from(e).context("could not deserialise worker response");
                        Err(error)
                    }
                };
                */

                println!("got: {ev:?}");
                println!("data: {:?}", ev.data());

                if let Err(e) = response_tx.try_send(from_value(ev.data()).context("could not convert response")) {
                    error!("message forwarding channel closed, should not happen: {e}");
                }
            })
        };
        let onmessage = Closure::new(onmessage_callback);
        port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        Self {
            port,
            response: rx,
            _onmessage: onmessage,
            _message: Default::default(),
        }
    }

    pub async fn recv(&mut self) -> Result<R> {
        self.response.recv().await.expect("forwarding channel shouldn't close")
    }

    pub(crate) async fn exec(&self, command: T) -> Result<R> {
        Err(Error::new("unimplemented"))
        //todo!("UNDONE"
            /*
        let mut response_channel = self.response.lock().await;

        self.send(command)?;

        response_channel.recv().await
            .expect("response channel should never be dropped")
    */
    }

    pub fn send(&self, message: &T) -> Result<()> {
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

    //wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn message_passing() {
        /*
        println!("foo");
        let channel = MessageChannel::new().unwrap();
        let p1 = channel.port1();
        let p2 = channel.port2();

        println!("foo");
        let client = RequestResponse::<u32, String>::new(p1);
        println!("foo");
        let mut server = RequestResponse::<String, u32>::new(p2);
        println!("foo");
        client.send(&1).unwrap();
        println!("foo");
        let recv_result = server.recv().await.unwrap();

        assert_eq!(recv_result, 1);
        */
    }
}
