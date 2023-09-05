use std::future::Future;
use std::pin::Pin;

use libp2p::swarm;

pub(crate) struct Executor;

impl swarm::Executor for Executor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        spawn(future)
    }
}

pub(crate) fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::spawn(future);
    }

    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(future);
    }
}
