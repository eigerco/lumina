use std::future::Future;

use tokio::{sync::watch, task::spawn_blocking};
use tokio_util::sync::CancellationToken;

use crate::executor::{spawn_cancellable, JoinHandle};

pub(crate) struct SpawnedTasks {
    cancellation_token: CancellationToken,
    counter_tx: watch::Sender<usize>,
    counter_rx: watch::Receiver<usize>,
}

struct DecreaseGuard(watch::Sender<usize>);

impl DecreaseGuard {
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
        self.counter_rx.wait_for(|counter| *counter == 0).await;
    }

    pub(crate) fn cancel_all(&mut self) {
        // Cancel all the ongoing tasks.
        self.cancellation_token.cancel();
        // Reset the token for tasks spawned later on.
        self.cancellation_token = CancellationToken::new();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[track_caller]
    pub(crate) fn spawn<F>(&self, fut: F) -> JoinHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.counter_tx.send_modify(|counter| *counter += 1);
        let decrease_guard = DecreaseGuard(self.counter_tx.clone());

        spawn_cancellable(self.cancellation_token.child_token(), async move {
            let _decrease_guard = decrease_guard;
            fut.await;
        })
    }

    #[cfg(target_arch = "wasm32")]
    #[track_caller]
    pub(crate) fn spawn<F>(&self, fut: F) -> JoinHandle
    where
        F: Future<Output = ()> + 'static,
    {
        self.counter_tx.send_modify(|counter| *counter += 1);
        let decrease_guard = DecreaseGuard(self.counter_tx.clone());

        spawn_cancellable(self.cancellation_token.child_token(), async move {
            let _decrease_guard = decrease_guard;
            fut.await;
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[track_caller]
    pub(crate) fn spawn_blocking<F, R>(&self, f: F) -> tokio::task::JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.counter_tx.send_modify(|counter| *counter += 1);
        let decrease_guard = DecreaseGuard(self.counter_tx.clone());

        spawn_blocking(move || {
            let _decrease_guard = decrease_guard;
            f()
        })
    }
}
