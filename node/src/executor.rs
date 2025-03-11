use std::fmt::{self, Debug};
use std::future::Future;

use tokio::select;
use tokio_util::sync::CancellationToken;

use crate::utils::Token;

#[allow(unused_imports)]
pub use self::imp::{sleep, spawn, spawn_cancellable, timeout, yield_now, Elapsed, Interval};

/// Naive `JoinHandle` implementation.
pub struct JoinHandle(Token);

impl JoinHandle {
    pub async fn join(&self) {
        self.0.triggered().await;
    }
}

impl Debug for JoinHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JoinHandle { .. }")
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use std::time::Duration;

    pub use tokio::time::error::Elapsed;
    pub use tokio::time::{sleep, timeout};

    #[track_caller]
    pub fn spawn<F>(future: F) -> JoinHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let token = Token::new();
        let guard = token.trigger_drop_guard();

        tokio::spawn(async move {
            let _guard = guard;
            future.await;
        });

        JoinHandle(token)
    }

    /// Spawn a cancellable task.
    ///
    /// This will cancel the task in the highest layer and should not be used
    /// if cancellation must happen in a point.
    #[track_caller]
    pub fn spawn_cancellable<F>(cancelation_token: CancellationToken, future: F) -> JoinHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let token = Token::new();
        let guard = token.trigger_drop_guard();

        tokio::spawn(async move {
            let _guard = guard;
            select! {
                // Run branches in order.
                biased;

                _ = cancelation_token.cancelled() => {}
                _ = future => {}
            }
        });

        JoinHandle(token)
    }

    pub struct Interval(tokio::time::Interval);

    impl Interval {
        pub async fn new(dur: Duration) -> Self {
            let mut inner = tokio::time::interval(dur);

            // In Tokio the first tick returns immediately, so we
            // consume to it to create an identical cross-platform
            // behavior.
            inner.tick().await;

            Interval(inner)
        }

        pub async fn tick(&mut self) {
            self.0.tick().await;
        }
    }

    pub use tokio::task::yield_now;
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

    pub fn spawn<F>(future: F) -> JoinHandle
    where
        F: Future<Output = ()> + 'static,
    {
        let token = Token::new();
        let guard = token.trigger_drop_guard();

        wasm_bindgen_futures::spawn_local(async move {
            let _guard = guard;
            future.await;
        });

        JoinHandle(token)
    }

    /// Spawn a cancellable task.
    ///
    /// This will cancel the task in the highest layer and should not be used
    /// if cancellation must happen in a point.
    pub fn spawn_cancellable<F>(cancelation_token: CancellationToken, future: F) -> JoinHandle
    where
        F: Future<Output = ()> + 'static,
    {
        let token = Token::new();
        let guard = token.trigger_drop_guard();

        wasm_bindgen_futures::spawn_local(async move {
            let _guard = guard;
            select! {
                // Run branches in order.
                biased;

                _ = cancelation_token.cancelled() => {}
                _ = future => {}
            }
        });

        JoinHandle(token)
    }

    pub struct Interval(SendWrapper<IntervalStream>);

    impl Interval {
        pub async fn new(dur: Duration) -> Self {
            // If duration was less than a millisecond, then make
            // it 1 millisecond.
            let millis = u32::try_from(dur.as_millis().max(1)).unwrap_or(u32::MAX);

            Interval(SendWrapper::new(IntervalStream::new(millis)))
        }

        pub async fn tick(&mut self) {
            self.0.next().await;
        }
    }

    #[derive(Debug)]
    pub struct Elapsed;

    pub fn timeout<F>(duration: Duration, future: F) -> Timeout<F>
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

    pub async fn sleep(duration: Duration) {
        let millis = u32::try_from(duration.as_millis().max(1)).unwrap_or(u32::MAX);
        let delay = SendWrapper::new(TimeoutFuture::new(millis));
        delay.await;
    }

    #[pin_project]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[derive(Debug)]
    pub struct Timeout<T> {
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
    pub async fn yield_now() {
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen]
            fn setTimeout(closure: &Closure<dyn FnMut()>, timeout: u32) -> i32;

            #[wasm_bindgen]
            fn clearTimeout(id: i32);
        }

        struct ClearTimeoutOnCancel(Option<i32>);

        impl ClearTimeoutOnCancel {
            fn disarm(mut self) {
                self.0.take();
            }
        }

        impl Drop for ClearTimeoutOnCancel {
            fn drop(&mut self) {
                if let Some(id) = self.0.take() {
                    clearTimeout(id);
                }
            }
        }

        let fut = async move {
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
            let id = setTimeout(&wake_closure, 0);
            let guard = ClearTimeoutOnCancel(Some(id));

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

            guard.disarm();
        };

        let fut = SendWrapper::new(fut);
        fut.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::async_test;
    use std::time::Duration;
    use web_time::Instant;

    #[async_test]
    async fn join_handle() {
        let now = Instant::now();

        let join_handle = spawn(async {
            sleep(Duration::from_millis(10)).await;
        });

        join_handle.join().await;
        assert!(now.elapsed() >= Duration::from_millis(10));

        // This must return immediately.
        join_handle.join().await;
    }
}
