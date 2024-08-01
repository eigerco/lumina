use std::future::Future;

use tokio::select;
use tokio_util::sync::CancellationToken;

#[allow(unused_imports)]
pub(crate) use self::imp::{
    sleep, spawn, spawn_cancellable, timeout, yield_now, Elapsed, Interval,
};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use std::time::Duration;

    pub(crate) use tokio::time::error::Elapsed;
    pub(crate) use tokio::time::{sleep, timeout};

    #[track_caller]
    pub(crate) fn spawn<F>(future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(future);
    }

    /// Spawn a cancellable task.
    ///
    /// This will cancel the task in the highest layer and should not be used
    /// if cancellation must happen in a point.
    #[track_caller]
    pub(crate) fn spawn_cancellable<F>(cancelation_token: CancellationToken, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            select! {
                _ = cancelation_token.cancelled() => {}
                _ = future => {}
            }
        });
    }

    pub(crate) struct Interval(tokio::time::Interval);

    impl Interval {
        pub(crate) async fn new(dur: Duration) -> Self {
            let mut inner = tokio::time::interval(dur);

            // In Tokio the first tick returns immediately, so we
            // consume to it to create an identical cross-platform
            // behavior.
            inner.tick().await;

            Interval(inner)
        }

        pub(crate) async fn tick(&mut self) {
            self.0.tick().await;
        }
    }

    pub(crate) use tokio::task::yield_now;
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::future::poll_fn;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    use futures::StreamExt;
    use gloo_timers::future::{IntervalStream, TimeoutFuture};
    use pin_project::pin_project;
    use send_wrapper::SendWrapper;
    use wasm_bindgen::prelude::*;

    use super::*;

    pub(crate) fn spawn<F>(future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        wasm_bindgen_futures::spawn_local(future);
    }

    /// Spawn a cancellable task.
    ///
    /// This will cancel the task in the highest layer and should not be used
    /// if cancellation must happen in a point.
    pub(crate) fn spawn_cancellable<F>(cancelation_token: CancellationToken, future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        wasm_bindgen_futures::spawn_local(async move {
            select! {
                _ = cancelation_token.cancelled() => {}
                _ = future => {}
            }
        });
    }

    pub(crate) struct Interval(SendWrapper<IntervalStream>);

    impl Interval {
        pub(crate) async fn new(dur: Duration) -> Self {
            // If duration was less than a millisecond, then make
            // it 1 millisecond.
            let millis = u32::try_from(dur.as_millis().max(1)).unwrap_or(u32::MAX);

            Interval(SendWrapper::new(IntervalStream::new(millis)))
        }

        pub(crate) async fn tick(&mut self) {
            self.0.next().await;
        }
    }

    #[derive(Debug)]
    pub(crate) struct Elapsed;

    pub(crate) fn timeout<F>(duration: Duration, future: F) -> Timeout<F>
    where
        F: Future,
    {
        let millis = u32::try_from(duration.as_millis().max(1)).unwrap_or(u32::MAX);
        let delay = SendWrapper::new(TimeoutFuture::new(millis));

        Timeout {
            value: future,
            delay,
        }
    }

    pub(crate) async fn sleep(duration: Duration) {
        let millis = u32::try_from(duration.as_millis().max(1)).unwrap_or(u32::MAX);
        let delay = SendWrapper::new(TimeoutFuture::new(millis));
        delay.await;
    }

    #[pin_project]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[derive(Debug)]
    pub(crate) struct Timeout<T> {
        #[pin]
        value: T,
        #[pin]
        delay: SendWrapper<TimeoutFuture>,
    }

    impl<T> Future for Timeout<T>
    where
        T: Future,
    {
        type Output = Result<T::Output, Elapsed>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let me = self.project();

            if let Poll::Ready(v) = me.value.poll(cx) {
                return Poll::Ready(Ok(v));
            }

            match me.delay.poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(Elapsed)),
                Poll::Pending => Poll::Pending,
            }
        }
    }

    /// Yields execution back to JavaScript's event loop
    pub(crate) async fn yield_now() {
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen]
            fn setTimeout(closure: &Closure<dyn FnMut()>, timeout: u32);
        }

        let yielded = Rc::new(Cell::new(false));
        let waker = Rc::new(RefCell::new(None::<Waker>));

        let wake_closure = {
            let yielded = yielded.clone();
            let waker = waker.clone();

            Closure::new(move || {
                yielded.set(true);
                waker.borrow_mut().take().unwrap().wake();
            })
        };

        // Unlike `queueMicrotask` or a naive yield_now implementation (i.e. `wake()`
        // and return `Poll::Pending` once), `setTimeout` closure will be executed by
        // JavaScript's event loop.
        //
        // This has two main benefits:
        //
        // * Garbage collector will be executed.
        // * We give time to JavaScript's tasks too.
        //
        // Ref: https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html
        setTimeout(&wake_closure, 0);

        debug_assert!(!yielded.get(), "Closure called before reaching event loop");

        poll_fn(|cx| {
            if yielded.get() {
                Poll::Ready(())
            } else {
                *waker.borrow_mut() = Some(cx.waker().to_owned());
                Poll::Pending
            }
        })
        .await;
    }
}
