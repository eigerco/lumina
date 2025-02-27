use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use tokio::sync::oneshot as tokio_oneshot;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent, MessagePort};

use crate::error::{Context, Error, Result};

pub(crate) fn new<T>() -> Result<(OneshotSender<T>, OneshotReceiver<T>)>
where
    T: Serialize + DeserializeOwned,
{
    let chan = MessageChannel::new()?;

    let tx = OneshotSender::new(chan.port1());
    let rx = OneshotReceiver::new(chan.port2());

    Ok((tx, rx))
}

#[derive(Serialize, Deserialize)]
enum Message<T> {
    Closed,
    Item(T),
}

pub(crate) struct OneshotReceiver<T: DeserializeOwned> {
    port: MessagePort,
    rx: Option<tokio_oneshot::Receiver<JsValue>>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _phantom: PhantomData<T>,
}

impl<T> OneshotReceiver<T>
where
    T: DeserializeOwned,
{
    pub(crate) fn new(port: MessagePort) -> Self {
        let (tx, rx) = tokio_oneshot::channel();

        let onmessage = Closure::once(move |ev: MessageEvent| {
            let _ = tx.send(ev.data());
        });

        port.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        OneshotReceiver {
            port,
            rx: Some(rx),
            _onmessage: onmessage,
            _phantom: PhantomData,
        }
    }

    pub(crate) async fn recv(mut self) -> Result<T> {
        let data = self.rx.take().unwrap().await.context("Channel closed")?;
        let msg = from_value(data).context("Deserialization failed")?;

        match msg {
            Message::Item(item) => Ok(item),
            Message::Closed => Err(Error::new("Channel closed")),
        }
    }
}

impl<T> Drop for OneshotReceiver<T>
where
    T: DeserializeOwned,
{
    fn drop(&mut self) {
        self.port.close();
    }
}

pub(crate) struct OneshotSender<T: Serialize> {
    port: Option<MessagePort>,
    _phantom: PhantomData<T>,
}

impl<T> OneshotSender<T>
where
    T: Serialize,
{
    pub(crate) fn new(port: MessagePort) -> Self {
        OneshotSender {
            port: Some(port),
            _phantom: PhantomData,
        }
    }

    fn send_msg(&mut self, msg: Message<T>) -> Result<()> {
        let data = to_value(&msg).context("Serialization failed")?;

        let port = self.port.take().expect("OneshotSender sends twice");
        port.post_message(&data).context("Sending failed")?;
        port.close();

        Ok(())
    }

    pub(crate) fn send(mut self, item: T) -> Result<()> {
        self.send_msg(Message::Item(item))
    }

    pub(crate) fn passive(mut self) -> PassiveOneshotSender<T> {
        PassiveOneshotSender {
            port: self.port.take().expect("OneshotSender already used"),
            _phantom: PhantomData,
        }
    }

    pub(crate) fn into_inner(mut self) -> MessagePort {
        self.port.take().expect("OneshotSender already used")
    }
}

impl<T> Drop for OneshotSender<T>
where
    T: Serialize,
{
    fn drop(&mut self) {
        if self.port.is_some() {
            let _ = self.send_msg(Message::Closed);
        }
    }
}

impl<T> From<PassiveOneshotSender<T>> for OneshotSender<T>
where
    T: Serialize,
{
    fn from(sender: PassiveOneshotSender<T>) -> OneshotSender<T> {
        sender.active()
    }
}

/// A passive oneshot sender.
///
/// This is used when we are transfering ownership of the underlying `MessagePort`
/// to a worker. The difference with the normal `OneshotSender` is the following:
///
/// * `PassiveOneshotSender` doesn't close `MessagePort` on drop.
/// * `PassiveOneshotSender` doesn't allow you to send a message.
/// * `PassiveOneshotSender` implements [`Serialize`]/[`Deserialize`].
#[derive(Serialize, Deserialize)]
pub(crate) struct PassiveOneshotSender<T: Serialize> {
    #[serde(with = "serde_wasm_bindgen::preserve")]
    port: MessagePort,
    #[serde(skip)]
    _phantom: PhantomData<T>,
}

impl<T> PassiveOneshotSender<T>
where
    T: Serialize,
{
    pub(crate) fn active(self) -> OneshotSender<T> {
        OneshotSender {
            port: Some(self.port),
            _phantom: PhantomData,
        }
    }
}

impl<T> Debug for PassiveOneshotSender<T>
where
    T: Serialize,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("PassiveOneshotSender { .. }")
    }
}

impl<T> From<OneshotSender<T>> for PassiveOneshotSender<T>
where
    T: Serialize,
{
    fn from(sender: OneshotSender<T>) -> PassiveOneshotSender<T> {
        sender.passive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::{self, Either};
    use gloo_timers::future::sleep;
    use std::pin::pin;
    use std::time::Duration;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    async fn smoke() {
        let (tx, rx) = super::new::<u64>().unwrap();

        let tx_js_val = to_value(&tx.passive()).unwrap();
        let tx: PassiveOneshotSender<u64> = from_value(tx_js_val).unwrap();

        tx.active().send(1234u64).unwrap();
        let val = rx.recv().await.unwrap();
        assert_eq!(val, 1234);
    }

    #[wasm_bindgen_test]
    async fn sender_drop() {
        let (tx, rx) = super::new::<u64>().unwrap();

        let tx_js_val = to_value(&tx.passive()).unwrap();
        let tx: PassiveOneshotSender<u64> = from_value(tx_js_val).unwrap();

        drop(tx.active());
        rx.recv().await.unwrap_err();
    }

    #[wasm_bindgen_test]
    async fn passive_sender_drop() {
        let (tx, rx) = super::new::<u64>().unwrap();

        let tx_js_val = to_value(&tx.passive()).unwrap();
        let tx: PassiveOneshotSender<u64> = from_value(tx_js_val).unwrap();

        let recv_fut = pin!(rx.recv());
        let timeout_fut = pin!(sleep(Duration::from_millis(100)));

        drop(tx);

        match future::select(recv_fut, timeout_fut).await {
            Either::Left(_) => panic!("PassiveOneshotSender sent `Closed` but it shouldn't"),
            Either::Right(_) => {}
        }
    }
}
