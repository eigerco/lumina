use std::fmt::{self, Debug};

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub(crate) struct SpawnedTasks {
    cancellation_token: CancellationToken,
    counter_tx: watch::Sender<usize>,
    counter_rx: watch::Receiver<usize>,
}

// We don't know if spawned tasks will ever be scheduled, so we need
// to decrease the counter with a drop guard.
struct DecreaseGuard(watch::Sender<usize>);

impl Drop for DecreaseGuard {
    fn drop(&mut self) {
        self.0.send_modify(|counter| *counter -= 1);
    }
}

impl SpawnedTasks {
    pub(crate) fn new() -> Self {
        let (counter_tx, counter_rx) = watch::channel(0);

        SpawnedTasks {
            cancellation_token: CancellationToken::new(),
            counter_tx,
            counter_rx,
        }
    }

    pub(crate) async fn wait_all(&mut self) {
        self.counter_rx
            .wait_for(|counter| *counter == 0)
            .await
            .expect("Channel is never closed");
    }

    pub(crate) fn cancel_all(&mut self) {
        self.cancellation_token.cancel();
        // Reset the token for tasks spawned later on.
        self.cancellation_token = CancellationToken::new();
    }

    #[track_caller]
    pub(crate) fn spawn_blocking<F, R>(&self, f: F) -> tokio::task::JoinHandle<Option<R>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.counter_tx.send_modify(|counter| *counter += 1);
        let decrease_guard = DecreaseGuard(self.counter_tx.clone());

        let cancellation_token = self.cancellation_token.child_token();

        tokio::task::spawn_blocking(move || {
            let _decrease_guard = decrease_guard;

            // If cancel was triggered before the closure was scheduled then do not run it.
            if cancellation_token.is_cancelled() {
                return None;
            }

            Some(f())
        })
    }
}

impl Debug for SpawnedTasks {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("SpawnedTasks { .. }")
    }
}
