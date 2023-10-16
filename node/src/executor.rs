use std::future::Future;
use std::pin::Pin;

use libp2p::swarm;

pub(crate) use self::imp::{spawn, yield_now, Interval};

pub(crate) struct Executor;

impl swarm::Executor for Executor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        spawn(future)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use std::time::Duration;

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

    pub(crate) use tokio::task::yield_now;
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use futures::StreamExt;
    use gloo_timers::future::IntervalStream;
    use send_wrapper::SendWrapper;
    use std::future::poll_fn;
    use std::task::Poll;
    use std::time::Duration;

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
            let millis = u32::try_from(dur.as_millis().max(1)).unwrap_or(u32::MAX);

            Interval(SendWrapper::new(IntervalStream::new(millis)))
        }

        pub(crate) async fn tick(&mut self) {
            self.0.next().await;
        }
    }

    pub(crate) async fn yield_now() {
        let mut yielded = false;

        poll_fn(|cx| {
            if yielded {
                return Poll::Ready(());
            }

            cx.waker().wake_by_ref();
            yielded = true;
            Poll::Pending
        })
        .await;
    }
}
