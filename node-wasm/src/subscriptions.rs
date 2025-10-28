use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, ready};

use futures::Stream;
use js_sys::{AsyncIterator, Boolean, Object, Promise, Reflect, Symbol};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::error::Result;
use crate::ports::{MessagePortLike, PortSender, RawPortReceier, split_port};
use crate::worker::SubscriptionFeedback;

#[wasm_bindgen(getter_with_clone)]
#[derive(Debug, Serialize, Deserialize)]
pub struct SubscriptionError {
    pub height: Option<u64>,
    pub error: String,
}

struct SubscriptionStream(Rc<RefCell<SubscriptionStreamInner>>);

struct SubscriptionStreamInner {
    receiver: RawPortReceier,
    feedback: PortSender<SubscriptionFeedback>,
}

impl SubscriptionStream {
    fn new(receiver: RawPortReceier, feedback: PortSender<SubscriptionFeedback>) -> Self {
        SubscriptionStream(Rc::new(RefCell::new(SubscriptionStreamInner {
            receiver,
            feedback,
        })))
    }

    fn send_ready(&self) -> Result<()> {
        self.0
            .borrow_mut()
            .feedback
            .send(&SubscriptionFeedback::Ready, &[])
    }
}

impl Future for SubscriptionStream {
    type Output = Option<JsValue>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let event = ready!(Pin::new(&mut self.0.borrow_mut().receiver).poll_next(cx));
        Poll::Ready(event.map(|ev| ev.data()))
    }
}

impl Clone for SubscriptionStream {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub(crate) fn into_async_iterator(port: MessagePortLike) -> Result<AsyncIterator> {
    let (feedback, receiver) = split_port(port)?;
    let stream = SubscriptionStream::new(receiver, feedback);

    let next = Closure::<dyn FnMut() -> Promise>::new(move || {
        let cloned = stream.clone();
        future_to_promise(async move {
            if let Err(e) = cloned.send_ready() {
                return Ok(to_value(&e).unwrap());
            }
            let next_item = cloned.await;

            let result = Object::new();
            Reflect::set(&result, &"done".into(), &Boolean::from(next_item.is_none()))
                .expect("reflect shouldn't fail on Object");
            if let Some(item) = next_item {
                Reflect::set(
                    &result,
                    &"value".into(),
                    &item, //&to_value(&item).expect("to_value shouldn't fail here"),
                )
                .expect("reflect shouldn't fail on Object");
            }
            Ok(result.into())
        })
    })
    .into_js_value();

    let iterator = Object::new();
    let iterator_self = iterator.clone();
    let iterator_return_this =
        Closure::<dyn FnMut() -> JsValue>::new(move || iterator_self.clone().into())
            .into_js_value();

    Reflect::set(&iterator, &"next".into(), &next).expect("reflect shouldn't fail on Object");
    Reflect::set(&iterator, &Symbol::async_iterator(), &iterator_return_this)
        .expect("reflect shouldn't fail on Object");
    Ok(iterator.unchecked_into())
}
