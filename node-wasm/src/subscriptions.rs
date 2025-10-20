use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use js_sys::{AsyncIterator, Boolean, Object, Promise, Reflect, Symbol};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::to_value;
use tokio::sync::mpsc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::Error;
use crate::ports::Port;
use crate::worker::SubscriptionFeedback;

#[wasm_bindgen(getter_with_clone)]
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionError {
    pub height: Option<u64>,
    pub error: String,
}

struct JsStream<S>(Rc<RefCell<JsStreamInner<S>>>);

impl<S> Clone for JsStream<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

struct JsStreamInner<S> {
    channel: mpsc::UnboundedReceiver<S>,
    port: Port,
}

impl<S> JsStream<S> {
    pub(crate) fn new(channel: mpsc::UnboundedReceiver<S>, port: Port) -> Self {
        JsStream(Rc::new(RefCell::new(JsStreamInner { channel, port })))
    }

    fn send_ready(&self) -> Result<(), Error> {
        self.0
            .borrow_mut()
            .port
            .send_raw(&SubscriptionFeedback::Ready)
    }
}

impl<S> Future for JsStream<S> {
    type Output = Option<S>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        Pin::new(&mut self.0.borrow_mut().channel).poll_recv(cx)
    }
}

pub(crate) fn into_async_iterator<S: Serialize + 'static>(
    channel: mpsc::UnboundedReceiver<Result<S, SubscriptionError>>,
    port: Port,
) -> AsyncIterator {
    let stream = JsStream::new(channel, port);
    let next = Closure::<dyn FnMut() -> Promise>::new(move || {
        let cloned = stream.clone();
        future_to_promise(async move {
            if let Err(e) = cloned.send_ready() {
                return Ok(to_value(&e).unwrap());
            }
            let next_item = match cloned.await.transpose() {
                Ok(item) => item,
                Err(e) => return Ok(to_value(&e).unwrap()),
            };

            let result = Object::new();
            Reflect::set(&result, &"done".into(), &Boolean::from(next_item.is_none()))
                .expect("reflect shouldn't fail on Object");
            if let Some(item) = next_item {
                Reflect::set(
                    &result,
                    &"value".into(),
                    &to_value(&item).expect("to_value shouldn't fail here"),
                )
                .expect("reflect shouldn't fail on Object");
            }
            Ok(result.into())
        })
    })
    .into_js_value();

    let iterator = Object::new();

    let iterator_self = iterator.clone();
    let async_iterator =
        Closure::<dyn FnMut() -> JsValue>::new(move || iterator_self.clone().into())
            .into_js_value();

    Reflect::set(&iterator, &"next".into(), &next).expect("reflect shouldn't fail on Object");
    Reflect::set(&iterator, &Symbol::async_iterator(), &async_iterator)
        .expect("reflect shouldn't fail on Object");
    iterator.unchecked_into()
}
