use js_sys::{Array, Function, Reflect};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::from_value;
use tokio::sync::mpsc;
use tracing::error;
use wasm_bindgen::prelude::*;
use web_sys::MessageEvent;

use crate::error::{Context, Error, Result};
use crate::utils::{to_json_value, MessageEventExt};

/// Counter-style message id for matching responses with requests
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
// Do _not_ use sizes larger than u32 here, as it doesn't play well
// with json serialisation that's happening for browser extension Port.
pub(crate) struct MessageId(u32);

/// Message being exchanged over the message port. Carries an id for identification plus payload
#[derive(Serialize, Deserialize)]
struct MultiplexMessage<T: Serialize> {
    /// Id of the message being sent. Id should not be re-used
    pub id: MessageId,
    /// Actual content of the message
    pub payload: Option<T>,
}

/// Message payload together with port being transferred, if applicable.
pub(crate) struct PayloadWithContext<T> {
    /// Id of the message being sent. Id should not be re-used
    pub id: MessageId,
    /// Actual content of the message
    pub payload: Option<T>,
    /// Port being transferred
    pub port: Option<MessagePortLike>,
}

// Instead of supporting communication with just `MessagePort`, allow using any object which
// provides compatible interface, eg. `Worker`
#[wasm_bindgen]
extern "C" {
    /// Abstraction over JavaScript MessagePort (but also runtime.Port for browser extension).
    /// Object which can `postMessage` and receive `onmessage` events.
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

/// Wraps JavaScript object with port-like semantics (can `postMessage` and receive `onmessage`),
/// with some convenience functions and type checking.
pub(crate) struct Port {
    port: MessagePortLike,
    onmessage: Closure<dyn Fn(MessageEvent)>,
}

impl MessageId {
    /// i++
    pub(crate) fn post_increment(&mut self) -> MessageId {
        let ret = *self;
        let (next, _carry) = self.0.overflowing_add(1);
        self.0 = next;
        ret
    }
}

impl Port {
    /// Create a new Port out of JS object, registering on message callback.
    /// Minimal duck-type checking is performed using reflection when setting appropriate properties.
    pub fn new<F>(object: JsValue, onmessage_callback: F) -> Result<Port>
    where
        F: Fn(MessageEvent) -> Result<()> + 'static,
    {
        let _post_message: Function = Reflect::get(&object, &"postMessage".into())?
            .dyn_into()
            .context("could not get object's postMessage")?;

        let onmessage = Closure::new(move |ev: MessageEvent| {
            if let Err(e) = onmessage_callback(ev) {
                error!("error receiving message: {e}");
            }
        });

        let port = register_onmessage_callback(object, &onmessage)?;

        Ok(Port { port, onmessage })
    }

    /// Create a new `Port` object, where received message are forwarded over the provided channel
    pub fn new_with_channels<T>(
        object: JsValue,
        forwarding_channel: mpsc::UnboundedSender<PayloadWithContext<T>>,
    ) -> Result<Port>
    where
        T: Serialize + DeserializeOwned + 'static,
    {
        tracing::info!("new port creation");
        Port::new(object, move |ev: MessageEvent| -> Result<()> {
            let port = ev.get_port();
            let MultiplexMessage { id, payload } =
                from_value(ev.data()).context("could not deserialize message")?;
            forwarding_channel
                .send(PayloadWithContext { id, payload, port })
                .context("forwarding failed, no receiver waiting")?;
            Ok(())
        })
    }

    /// Send a raw serialisable message over the port. No checking is performed whether the
    /// receiver is able to correctly interpret the message.
    pub fn send_raw<T: Serialize>(&self, msg: &T) -> Result<()> {
        let value = to_json_value(&msg).context("error converting to JsValue")?;
        self.port
            .post_message(&value)
            .context("could not send message")?;
        Ok(())
    }

    /// Send a serialisable message over the port, wrapping it with MultiplexMessage.
    /// No checking is performed whether receiver is able to correctly interpret the message
    pub fn send<T: Serialize>(&self, id: MessageId, payload: T) -> Result<()> {
        let msg = MultiplexMessage {
            id,
            payload: Some(payload),
        };
        self.send_raw(&msg)
    }

    /// Send a serialisable message over the port, wrapped with MultiplexMessage, together
    /// with a object to transfer. No checking is performed whether receiver is able to correctly
    /// interpret the message, nor whether port can actually perform object transfer.
    pub fn send_with_transferable<T: Serialize>(
        &self,
        id: MessageId,
        payload: T,
        transferable: JsValue,
    ) -> Result<()> {
        let msg = MultiplexMessage {
            id,
            payload: Some(payload),
        };
        let value = to_json_value(&msg).context("error converting to JsValue")?;
        self.port
            .post_message_with_transferable(&value, &Array::of1(&transferable))
            .context("could not send message")?;
        Ok(())
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        unregister_onmessage(&self.port, &self.onmessage)
    }
}

// helper to hide slight differences in message passing between runtime.Port used by browser
// extensions and everything else
pub(crate) fn register_onmessage_callback<F>(
    object: JsValue,
    callback: &Closure<F>,
) -> Result<MessagePortLike, Error>
where
    F: Fn(MessageEvent) + ?Sized,
{
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
    use wasm_bindgen_test::wasm_bindgen_test;

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
}
