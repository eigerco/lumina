use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context as TaskContext, Poll, ready};

use futures::Stream;
use js_sys::{Array, Function, Reflect};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::from_value;
use tokio::sync::mpsc;
use tracing::error;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, MessagePort};

use crate::commands::{Command, WorkerCommand, WorkerResponse, WorkerResult};
use crate::error::{Context, Error, Result};
use crate::utils::{MessageEventExt, to_json_value};

/// Counter-style message id for matching responses with requests
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
// Do _not_ use sizes larger than u32 here, as it doesn't play well
// with json serialisation that's happening for browser extension Port.
pub(crate) struct MessageId(u32);

impl MessageId {
    /// i++
    pub(crate) fn post_increment(&mut self) -> MessageId {
        let ret = *self;
        let (next, _carry) = self.0.overflowing_add(1);
        self.0 = next;
        ret
    }
}

/// Message being exchanged over the message port. Carries an id for identification plus payload
#[derive(Serialize, Deserialize)]
pub(crate) struct MultiplexMessage<T: Serialize> {
    /// Id of the message being sent. Id should not be re-used
    pub id: MessageId,
    /// Actual content of the message
    pub payload: T,
}

impl TryFrom<MessageEvent> for MultiplexMessage<Command> {
    type Error = Error;

    fn try_from(ev: MessageEvent) -> Result<Self, Self::Error> {
        let MultiplexMessage::<Command> { id, mut payload } =
            from_value(ev.data()).context("could not deserialize message")?;

        if let Some(port) = ev.get_ports().into_iter().take(1).last() {
            if let Command::Management(WorkerCommand::ConnectPort(maybe_port)) = &mut payload {
                let _ = maybe_port.insert(port);
            }
        };

        Ok(MultiplexMessage { id, payload })
    }
}

impl TryFrom<MessageEvent> for MultiplexMessage<WorkerResult> {
    type Error = Error;

    fn try_from(ev: MessageEvent) -> Result<Self, Self::Error> {
        let MultiplexMessage::<WorkerResult> { id, mut payload } =
            from_value(ev.data()).context("could not deserialize message")?;

        if let Some(port) = ev.get_ports().into_iter().take(1).last() {
            if let Ok(WorkerResponse::Subscribed(maybe_port)) = &mut payload {
                let _ = maybe_port.insert(port);
            }
        };

        Ok(MultiplexMessage { id, payload })
    }
}

// Instead of supporting communication with just `MessagePort`, allow using any object which
// provides compatible interface, eg. `Worker`
#[wasm_bindgen]
extern "C" {
    /// Abstraction over JavaScript MessagePort (but also runtime.Port for browser extension).
    /// Object which can `postMessage` and receive `onmessage` events.
    #[derive(Debug)]
    pub(crate) type MessagePortLike;

    #[wasm_bindgen(catch, method, structural, js_name = postMessage)]
    fn post_message(this: &MessagePortLike, message: &JsValue) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, method, structural, js_name = postMessage)]
    pub fn post_message_with_transferable(
        this: &MessagePortLike,
        message: &JsValue,
        transferable: &JsValue,
    ) -> Result<(), JsValue>;
}

impl From<MessagePort> for MessagePortLike {
    fn from(value: MessagePort) -> Self {
        MessagePortLike { obj: value.into() }
    }
}

pub(crate) fn split_port<Tx>(port: MessagePortLike) -> Result<(PortSender<Tx>, RawPortReceier)> {
    let _post_message: Function = Reflect::get(&port, &"postMessage".into())?
        .dyn_into()
        .context("could not get object's postMessage")?;

    let port = Rc::new(port);

    let (tx, receiving_channel) = mpsc::unbounded_channel();
    let onmessage = Closure::new(move |ev: MessageEvent| {
        if tx.send(ev).is_err() {
            error!("forwarding message from port failed, no receiver waiting");
        }
    });
    register_onmessage_callback(port.as_ref(), &onmessage)?;

    let sender = PortSender::<Tx> {
        port: port.clone(),
        _sent: PhantomData,
    };
    let receiver = RawPortReceier {
        port,
        receiving_channel,
        onmessage,
    };
    Ok((sender, receiver))
}

pub(crate) struct PortSender<Tx> {
    port: Rc<MessagePortLike>,
    _sent: PhantomData<Tx>,
}

impl<Tx> PortSender<Tx>
where
    Tx: Serialize,
{
    pub(crate) fn send(&self, value: &Tx, ports: &[MessagePortLike]) -> Result<()> {
        let value = to_json_value(&value).context("error converting to JsValue")?;
        let ports: Array = ports.iter().collect();

        self.port
            .post_message_with_transferable(&value, &ports)
            .context("could not send message")
    }
}

pub(crate) struct RawPortReceier {
    port: Rc<MessagePortLike>,
    receiving_channel: mpsc::UnboundedReceiver<MessageEvent>,
    onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl Stream for RawPortReceier {
    type Item = MessageEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let event =
            ready!(this.receiving_channel.poll_recv(cx)).expect("forward channel should not drop");

        // TODO: proper channel shutdown
        Poll::Ready(Some(event))
    }
}

impl Drop for RawPortReceier {
    fn drop(&mut self) {
        unregister_onmessage(self.port.as_ref(), &self.onmessage);
    }
}

// helper to hide slight differences in message passing between runtime.Port used by browser
// extensions and everything else
pub(crate) fn register_onmessage_callback<F>(
    port: &MessagePortLike,
    callback: &Closure<F>,
) -> Result<(), Error>
where
    F: Fn(MessageEvent) + ?Sized,
{
    if Reflect::has(port, &JsValue::from("onMessage"))
        .context("failed to reflect onMessage property")?
    {
        // Browser extension runtime.Port has `onMessage` property, on which we should call
        // `addListener` on.
        let listeners = Reflect::get(port, &"onMessage".into())
            .context("could not get `onMessage` property")?;

        let add_listener: Function = Reflect::get(&listeners, &"addListener".into())
            .context("could not get `onMessage.addListener` property")?
            .dyn_into()
            .context("expected `onMessage.addListener` to be a function")?;
        Reflect::apply(&add_listener, &listeners, &Array::of1(callback.as_ref()))
            .context("error calling `onMessage.addListener`")?;
    } else if Reflect::has(port, &JsValue::from("onmessage"))
        .context("failed to reflect onmessage property")?
    {
        // MessagePort, as well as message passing via Worker instance, requires setting
        // `onmessage` property to callback
        Reflect::set(port, &"onmessage".into(), callback.as_ref())
            .context("could not set onmessage callback")?;
    } else {
        return Err(Error::new("Don't know how to register onmessage callback"));
    }

    Ok(())
}

/// unregister onmessage callback in any of the various forms it can take
pub(crate) fn unregister_onmessage(port: &JsValue, callback: &Closure<dyn Fn(MessageEvent)>) {
    if Reflect::has(port, &"onMessage".into()).unwrap_or_default() {
        // `runtime.Port` object. Unregistering callback with `removeListener`.
        let listeners =
            Reflect::get(port, &"onMessage".into()).expect("onMessage existence already checked");

        if let Ok(rm_listener) = Reflect::get(&listeners, &"removeListener".into())
            .and_then(|x| x.dyn_into::<Function>())
        {
            let _ = Reflect::apply(&rm_listener, &listeners, &Array::of1(callback.as_ref()));
        }
    } else if Reflect::has(port, &"onmessage".into()).unwrap_or_default() {
        // `MessagePort` object. Unregistering callback by setting `onmessage` to `null`.
        let _ = Reflect::set(port, &"onmessage".into(), &JsValue::NULL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::StreamExt;
    use futures::channel::oneshot;
    use lumina_utils::executor::spawn;
    use wasm_bindgen_futures::spawn_local;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    use crate::commands::{Command, CommandWithResponder, WorkerCommand, WorkerResponse};
    use crate::worker_client::WorkerClient;
    use crate::worker_server::WorkerServer;

    #[wasm_bindgen_test]
    fn message_id_increment() {
        let mut m = MessageId::default();
        assert_eq!(m.post_increment(), MessageId(0));
        assert_eq!(m.post_increment(), MessageId(1));
        assert_eq!(m.post_increment(), MessageId(2));
    }

    #[wasm_bindgen_test]
    fn message_id_overflow() {
        let mut m = MessageId(u32::MAX - 2);
        assert_eq!(m.post_increment(), MessageId(u32::MAX - 2));
        assert_eq!(m.post_increment(), MessageId(u32::MAX - 1));
        assert_eq!(m.post_increment(), MessageId(u32::MAX));
        assert_eq!(m.post_increment(), MessageId(0));
        assert_eq!(m.post_increment(), MessageId(1));
    }

    #[wasm_bindgen_test]
    async fn port_smoke() {
        let ch = MessageChannel::new().unwrap();
        let (tx0, _rx0) = split_port(ch.port1().into()).unwrap();
        let (_tx1, rx1) = split_port::<()>(ch.port2().into()).unwrap();
        let mut rx1 = rx1.map(|e| {
            let v: u32 = from_value(e.data()).unwrap();
            let p = e.get_ports();
            (v, p)
        });
        let transferred = MessageChannel::new().unwrap().port1().into();

        tx0.send(&1u32, &[transferred]).unwrap();
        let (value, ports) = rx1.next().await.unwrap();
        assert_eq!(value, 1);
        assert_eq!(ports.len(), 1);
    }

    #[wasm_bindgen_test]
    async fn worker_client_server() {
        let channel0 = MessageChannel::new().unwrap();
        let mut server = WorkerServer::new();
        let port_channel = server.get_port_channel();
        let (stop_tx, stop_rx) = oneshot::channel();

        spawn_local(async move {
            let CommandWithResponder { command, responder } = server.recv().await.unwrap();
            assert!(matches!(
                command,
                Command::Management(WorkerCommand::IsRunning)
            ));
            responder
                .send(Ok(WorkerResponse::IsRunning(false)))
                .unwrap();

            let CommandWithResponder { command, responder } = server.recv().await.unwrap();
            let Command::Management(WorkerCommand::ConnectPort(Some(port))) = command else {
                panic!("received unexpected command")
            };
            server.spawn_connection_worker(port).unwrap();
            responder.send(Ok(WorkerResponse::Ok)).unwrap();

            let CommandWithResponder { command, responder } = server.recv().await.unwrap();
            assert!(matches!(
                command,
                Command::Management(WorkerCommand::IsRunning)
            ));
            responder.send(Ok(WorkerResponse::IsRunning(true))).unwrap();

            stop_rx.await.unwrap(); // wait for the test to finish before shutting the server
        });

        port_channel.send(channel0.port1().into()).unwrap();
        let client0 = WorkerClient::new(channel0.port2().into()).unwrap();

        let response = client0.worker(WorkerCommand::IsRunning).await.unwrap();
        assert!(matches!(response, WorkerResponse::IsRunning(false)));

        let channel1 = MessageChannel::new().unwrap();
        client0
            .worker(WorkerCommand::ConnectPort(Some(channel1.port1().into())))
            .await
            .unwrap();
        let client1 = WorkerClient::new(channel1.port2().into()).unwrap();

        let response = client1.worker(WorkerCommand::IsRunning).await.unwrap();
        assert!(matches!(response, WorkerResponse::IsRunning(true)));
        stop_tx.send(()).unwrap();
    }

    #[wasm_bindgen_test]
    async fn spawn_client_server() {
        let channel = MessageChannel::new().unwrap();
        let client = WorkerClient::new(channel.port1().into()).unwrap();
        let (stop_tx, stop_rx) = oneshot::channel();

        spawn(async move {
            let mut server = WorkerServer::new();
            server
                .spawn_connection_worker(channel.port2().into())
                .unwrap();

            let CommandWithResponder { command, responder } = server.recv().await.unwrap();
            assert!(matches!(
                command,
                Command::Management(WorkerCommand::InternalPing)
            ));
            responder.send(Ok(WorkerResponse::InternalPong)).unwrap();
            stop_rx.await.unwrap(); // wait for the test to finish before shutting the server
        });

        let response = client.worker(WorkerCommand::InternalPing).await.unwrap();
        assert!(matches!(response, WorkerResponse::InternalPong));
        stop_tx.send(()).unwrap();
    }
}
