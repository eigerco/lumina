use std::fmt::{self, Debug};
use std::pin::pin;
use std::sync::Arc;

use tokio::sync::Notify;

pub(crate) struct Counter {
    counter: Arc<()>,
    notify: Arc<Notify>,
}

pub(crate) struct CounterGuard {
    counter: Option<Arc<()>>,
    notify: Arc<Notify>,
}

impl Drop for CounterGuard {
    fn drop(&mut self) {
        self.counter.take();
        self.notify.notify_waiters();
    }
}

impl Counter {
    pub(crate) fn new() -> Counter {
        Counter {
            counter: Arc::new(()),
            notify: Arc::new(Notify::new()),
        }
    }

    pub(crate) fn guard(&self) -> CounterGuard {
        CounterGuard {
            counter: Some(self.counter.clone()),
            notify: self.notify.clone(),
        }
    }

    /// Wait all guards to drop.
    pub(crate) async fn wait_guards(&mut self) {
        let mut notified = pin!(self.notify.notified());

        while Arc::strong_count(&self.counter) > 1 {
            notified.as_mut().await;
            notified.set(self.notify.notified());
        }
    }
}

impl Debug for Counter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Counter { .. }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tokio::spawn;
    use tokio::time::{sleep, timeout};

    #[tokio::test]
    async fn counter_works() {
        let mut counter = Counter::new();
        counter.wait_guards().await;

        let guard1 = counter.guard();
        let guard2 = counter.guard();
        let now = Instant::now();

        spawn(async move {
            let _guard = guard1;
            sleep(Duration::from_millis(100)).await;
        });

        spawn(async move {
            let _guard = guard2;
            sleep(Duration::from_millis(200)).await;
        });

        timeout(Duration::from_millis(300), counter.wait_guards())
            .await
            .unwrap();

        let elapsed = now.elapsed();
        assert!(elapsed >= Duration::from_millis(200) && elapsed < Duration::from_millis(300));
    }
}
