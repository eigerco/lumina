pub use self::imp::{
    sleep, timeout, Elapsed, Instant, Interval, Sleep, SystemTime, SystemTimeError, Timeout,
};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::time::Duration;

    pub use std::time::{Instant, SystemTime, SystemTimeError};
    pub use tokio::time::error::Elapsed;
    pub use tokio::time::{sleep, timeout, Sleep, Timeout};

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

    pub use web_time::{Instant, SystemTime, SystemTimeError};

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

    /// Future returned by `sleep`.
    #[pin_project]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[derive(Debug)]
    pub struct Sleep {
        #[pin]
        deadline: Instant,
        #[pin]
        fut: Option<SendWrapper<TimeoutFuture>>,
    }

    impl Future for Sleep {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
            let mut this = self.project();

            loop {
                if let Some(fut) = this.fut.as_mut().as_pin_mut() {
                    if fut.poll(cx).is_pending() {
                        return Poll::Pending;
                    }
                }

                let now = Instant::now();

                if now >= *this.deadline {
                    this.fut.take();
                    return Poll::Ready(());
                }

                // `Sleep` guarantees that it will sleep **at least** an X amount of time.
                // When we convert a duration to milliseconds there is a possibility to
                // saturate some sub-milliseconds amount. Therefore, to satisfy the sleep
                // guarantees, we add 1 millisecond more.
                let remaining_millis = this.deadline.duration_since(now).as_millis() + 1;

                // JavaScript's `set_timeout` can not take a delay bigger than `i32::MAX`.
                let delay_millis = remaining_millis.min(i32::MAX as u128);
                *this.fut = Some(SendWrapper::new(TimeoutFuture::new(delay_millis as u32)));

                // Loop again to register the new future.
            }
        }
    }

    /// Waits until `duration` has elapsed.
    pub fn sleep(duration: Duration) -> Sleep {
        Sleep {
            deadline: Instant::now() + duration,
            fut: None,
        }
    }

    /// This error is returned when a timeout expires before the function was able to finish.
    #[derive(Debug)]
    pub struct Elapsed;

    /// Future returned by `timeout`.
    #[pin_project]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[derive(Debug)]
    pub struct Timeout<T> {
        #[pin]
        value: T,
        #[pin]
        timeout: Sleep,
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

            match me.timeout.poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(Elapsed)),
                Poll::Pending => Poll::Pending,
            }
        }
    }

    /// Requires a `Future` to complete before the specified duration has elapsed.
    pub fn timeout<F>(duration: Duration, future: F) -> Timeout<F>
    where
        F: Future,
    {
        Timeout {
            value: future,
            timeout: sleep(duration),
        }
    }
}
