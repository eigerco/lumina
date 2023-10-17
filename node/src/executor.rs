use std::future::Future;
use std::pin::Pin;

use libp2p::swarm;

#[allow(unused_imports)]
pub(crate) use self::imp::{sleep, spawn, timeout, Elapsed, Interval};

pub(crate) struct Executor;

impl swarm::Executor for Executor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        spawn(future)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::time::Duration;
    pub(crate) use tokio::time::error::Elapsed;
    pub(crate) use tokio::time::{sleep, timeout};

    use super::*;

    pub(crate) fn spawn<F>(future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(future);
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
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use futures::StreamExt;
    use gloo_timers::future::IntervalStream;
    use gloo_timers::future::TimeoutFuture;
    use pin_project::pin_project;
    use send_wrapper::SendWrapper;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    pub(crate) use gloo_timers::future::sleep;

    pub(crate) fn spawn<F>(future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        wasm_bindgen_futures::spawn_local(future);
    }

    pub(crate) struct Interval(SendWrapper<IntervalStream>);

    impl Interval {
        pub(crate) async fn new(dur: Duration) -> Self {
            // If duration was less than a millisecond, then make
            // it 1 millisecond.
            let millis = u32::try_from(dur.as_millis().max(100)).unwrap_or(u32::MAX);

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
        let delay = TimeoutFuture::new(millis);

        Timeout {
            value: future,
            delay,
        }
    }

    #[pin_project]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[derive(Debug)]
    pub(crate) struct Timeout<T> {
        #[pin]
        value: T,
        #[pin]
        delay: TimeoutFuture,
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
}
