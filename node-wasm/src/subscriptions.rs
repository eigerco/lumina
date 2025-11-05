use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use futures::stream::LocalBoxStream;
use futures::{Stream, StreamExt};
use js_sys::{AsyncIterator, Boolean, Object, Promise, Reflect, Symbol};
use lumina_node::node::subscriptions::SubscriptionError;
use lumina_utils::executor::spawn;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use tracing::{debug, error, trace, warn};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use web_sys::{MessageChannel, MessagePort};

use crate::error::{Context as _, Error, Result};
use crate::ports::{MessagePortLike, PortSender, RawPortReceier, split_port};
use crate::utils::MessageChannelExt;

/// Error thrown while processing subscription
#[wasm_bindgen(getter_with_clone, js_name = "SubscriptionError")]
#[derive(Debug, Serialize, Deserialize)]
pub struct JsSubscriptionError {
    /// height at which the error occured, if applicable
    pub height: Option<u64>,
    /// error message
    pub error: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct SubscriptionReceiverReady;

struct SubscriptionStream<S>(Rc<RefCell<SubscriptionStreamInner<S>>>);

struct SubscriptionStreamInner<S> {
    receiver: LocalBoxStream<'static, Result<S, JsSubscriptionError>>,
    feedback: PortSender<SubscriptionReceiverReady>,
}

impl<S> SubscriptionStream<S>
where
    S: DeserializeOwned,
{
    fn new(receiver: RawPortReceier, feedback: PortSender<SubscriptionReceiverReady>) -> Self {
        let receiver = receiver
            .map(|result| {
                from_value::<Result<S, JsSubscriptionError>>(result.data()).map_err(|e| {
                    JsSubscriptionError {
                        height: None,
                        error: format!("error deserializing subscription item: {e}"),
                    }
                })?
            })
            .boxed_local();
        SubscriptionStream::<S>(Rc::new(RefCell::new(SubscriptionStreamInner {
            receiver,
            feedback,
        })))
    }

    fn send_ready(&self) -> Result<()> {
        self.0
            .borrow_mut()
            .feedback
            .send(&SubscriptionReceiverReady, &[])
    }
}

impl<S> Stream for SubscriptionStream<S>
where
    S: std::fmt::Debug,
{
    type Item = Result<S, JsSubscriptionError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0.borrow_mut().receiver).poll_next(cx)
    }
}

impl<S> Clone for SubscriptionStream<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[wasm_bindgen]
#[derive(Clone)]
struct ThisReturner {
    next: Rc<Closure<dyn FnMut() -> Promise>>,
}

#[wasm_bindgen]
impl ThisReturner {
    fn new(next: Closure<dyn FnMut() -> Promise>) -> ThisReturner {
        ThisReturner {
            next: Rc::new(next),
        }
    }

    pub fn next(&self) -> Promise {
        Reflect::apply(
            JsCast::unchecked_ref(self.next.as_ref().as_ref()),
            &JsValue::UNDEFINED,
            &js_sys::Array::new(),
        )
        .expect("apply on closure should succeed")
        .unchecked_into()
    }

    pub fn return_self(self) -> JsValue {
        JsValue::from(self.clone())
    }
}

// Wrap provided port into SubscriptionStream and prepare it to be used as
// js AsyncIterator. Assumes provided port was prepared with [`forward_stream_to_message_port`]
pub(crate) fn into_async_iterator<S>(port: MessagePortLike) -> Result<AsyncIterator>
where
    S: DeserializeOwned + Into<JsValue> + std::fmt::Debug + 'static,
{
    let (feedback, receiver) = split_port(port)?;
    let stream = SubscriptionStream::<S>::new(receiver, feedback);

    let next = Closure::<dyn FnMut() -> Promise>::new(move || {
        let mut cloned = stream.clone();
        future_to_promise(async move {
            if let Err(e) = cloned.send_ready() {
                return Ok(to_value(&e).unwrap());
            }
            let next_item = match cloned.next().await.transpose() {
                Ok(item) => item,
                Err(e) => {
                    // TODO: format the object
                    return Ok(to_value(&e).expect("conversion to work"));
                }
            };

            let result = Object::new();
            Reflect::set(&result, &"done".into(), &Boolean::from(next_item.is_none()))
                .expect("reflect shouldn't fail on Object");
            if let Some(item) = next_item {
                Reflect::set(&result, &"value".into(), &item.into())
                    .expect("reflect shouldn't fail on Object");
            }
            Ok(result.into())
        })
    });

    let iterator: JsValue = ThisReturner::new(next).into();

    let return_self_method =
        Reflect::get(&iterator, &"return_self".into()).expect("method should be present");
    Reflect::set(&iterator, &Symbol::async_iterator(), &return_self_method)
        .expect("reflect shouldn't fail on Object");

    Ok(iterator.unchecked_into())
}

// spawn a task responsible for sending items from the provided stream as they become available
// and as the receiving end signals its readiness with SubscriptionReceiverReady.
pub(crate) fn forward_stream_to_message_port<T>(
    mut stream: impl Stream<Item = Result<T, SubscriptionError>> + Unpin + 'static,
) -> Result<MessagePort>
where
    T: Serialize + Unpin + 'static,
{
    let (p0, p1) = MessageChannel::new_ports()?;

    let (subscription_sender, event_receiver) = split_port(p0.into())?;
    let mut feedback_receiver = event_receiver
        .map(|ev| from_value(ev.data()).context("could not deserialize subscription signal"));

    spawn(async move {
        trace!("Starting subscription");
        loop {
            let Some(feedback) = feedback_receiver.next().await else {
                break;
            };
            match feedback {
                Ok(SubscriptionReceiverReady) => (),
                Err(e) => {
                    warn!("Error receiving subscription feedback: {e}");
                }
            }
            let item: Result<Option<T>> = stream.next().await.transpose().map_err(Error::from);

            if let Err(e) = subscription_sender.send(&item, &[]) {
                error!("Error sending subscription item: {e}");
            }
        }
        debug!("Ending subscription");
    });

    Ok(p1)
}

#[cfg(test)]
mod tests {
    use crate::utils::MessageChannelExt;

    use super::*;

    use futures::StreamExt;
    use js_sys::Array;
    use lumina_utils::executor::spawn;
    use serde_wasm_bindgen::from_value;
    use wasm_bindgen_test::*;
    use web_sys::MessageChannel;

    #[wasm_bindgen(module = "/test/async_iterator.js")]
    extern "C" {
        async fn drain_async_iterator(iterator: AsyncIterator) -> Array;
    }

    #[wasm_bindgen_test]
    async fn smoke() {
        let (p0, p1) = MessageChannel::new_ports().unwrap();

        let (tx, rx) = split_port(p0.into()).unwrap();
        let mut rx = rx.map(|ev| from_value(ev.data()).unwrap());
        let iterator = into_async_iterator::<String>(p1.into()).unwrap();

        spawn(async move {
            let msg: Result<String, JsSubscriptionError> = Ok("hello".to_string());
            tx.send(&msg, &[]).unwrap();
            let feedback = rx.next().await;
            assert_eq!(feedback, Some(SubscriptionReceiverReady));

            let msg: Result<String, JsSubscriptionError> = Ok("world".to_string());
            tx.send(&msg, &[]).unwrap();
            let feedback = rx.next().await;
            assert_eq!(feedback, Some(SubscriptionReceiverReady));

            drop(tx)
        });

        let received: Vec<_> = drain_async_iterator(iterator)
            .await
            .iter()
            .map(|v| from_value::<String>(v).unwrap())
            .collect();
        assert_eq!(received.as_ref(), ["hello", "world"]);
    }

    #[wasm_bindgen_test]
    async fn close() {
        let (p0, p1) = MessageChannel::new_ports().unwrap();

        let (tx, rx) = split_port::<String>(p0.into()).unwrap();
        let async_iterator = into_async_iterator::<String>(p1.into());
        drop(async_iterator);

        tx.send(&"foo".to_string(), &[]).unwrap();

        let mut rx = rx.map(|ev| from_value::<SubscriptionReceiverReady>(ev.data()).unwrap());
        // at this point receiver would have already signaled its readiness,
        // so we need to clear the signal from the channel first.
        //assert_eq!(rx.next().await.unwrap(), SubscriptionReceiverReady);
        assert!(rx.next().await.is_none());
    }
}
