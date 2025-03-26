pub use self::imp::{sleep, timeout, Elapsed, Interval};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::time::Duration;

    pub use tokio::time::error::Elapsed;
    pub use tokio::time::{sleep, timeout};

    /// Type allowing to wait on a sequence of instants with a certain duration between each instant.
    pub struct Interval(tokio::time::Interval);

    impl Interval {
        /// Create a new `Interval` with provided duration between firings
        pub async fn new(dur: Duration) -> Self {
            let mut inner = tokio::time::interval(dur);

            // In Tokio the first tick returns immediately, so we
            // consume to it to create an identical cross-platform
            // behavior.
            inner.tick().await;

            Interval(inner)
        }

        /// Completes when the next instant in the interval has been reached.
        pub async fn tick(&mut self) {
            self.0.tick().await;
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use futures::StreamExt;
    use gloo_timers::future::{IntervalStream, TimeoutFuture};
    use pin_project::pin_project;
    use send_wrapper::SendWrapper;

    /// Type allowing to wait on a sequence of instants with a certain duration between each instant.
    pub struct Interval(SendWrapper<IntervalStream>);

    impl Interval {
        /// Create a new `Interval` with provided duration between firings
        pub async fn new(dur: Duration) -> Self {
            // If duration was less than a millisecond, then make
            // it 1 millisecond.
            let millis = u32::try_from(dur.as_millis().max(1)).unwrap_or(u32::MAX);

            Interval(SendWrapper::new(IntervalStream::new(millis)))
        }

        /// Completes when the next instant in the interval has been reached.
        pub async fn tick(&mut self) {
            self.0.next().await;
        }
    }

    /// This error is returned when a timeout expires before the function was able to finish.
    #[derive(Debug)]
    pub struct Elapsed;

    /// Requires a Future to complete before the specified duration has elapsed.
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

    /// Waits until `duration` has elapsed
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
}
