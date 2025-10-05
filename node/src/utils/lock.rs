// TODO: upstream to `named_lock`
// TODO: lock unlocking doesn't actually happen until rust yields to js.
// This can cause it to be held longer than expected, or require calling `yield_now` manually.

use std::fmt;

use js_sys::{Function, Object, Promise, Reflect};
use lumina_utils::make_object;
use lumina_utils::token::{Token, TokenTriggerDropGuard};
use serde_wasm_bindgen::to_value;
use tokio::sync::oneshot;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Call would block")]
    WouldBlock,

    #[error("Could not acquire lock manager")]
    LockManagerUnavailable(JsError),
}

pub struct NamedLock {
    _unlock_guard: TokenTriggerDropGuard,
}

impl NamedLock {
    pub async fn try_lock(name: &str) -> Result<NamedLock, Error> {
        NamedLock::lock_impl(name, false).await
    }

    #[cfg(test)]
    pub async fn lock(name: &str) -> Result<NamedLock, Error> {
        NamedLock::lock_impl(name, true).await
    }

    async fn lock_impl(name: &str, block: bool) -> Result<NamedLock, Error> {
        let lock_manager = get_lock_manager().map_err(Error::LockManagerUnavailable)?;

        let unlock_token = Token::new();
        let unlock_token_guard = unlock_token.trigger_drop_guard();
        let (would_block_tx, would_block_rx) = oneshot::channel();
        // Following callback will be called when:
        // - ifAvailable = true => immediately upon lock request. In this case `lock` will be
        //   null if the lock is currently being held.
        // - otherwise => when the lock is granted
        let cb: Function = Closure::once_into_js(move |lock: JsValue| {
            future_to_promise(async move {
                // falsy value here can happen if and only if we were called in non-blocking
                // mode and the lock is currently held.
                let _ = would_block_tx.send(lock.is_falsy());
                // The lock (if we acquire it), will be held until this promise is resolved,
                // effectively keeping it until `unlock_token` is triggered.
                unlock_token.triggered().await;
                Ok(JsValue::null())
            })
        })
        .unchecked_into();

        let opts = if block {
            Object::new()
        } else {
            make_object!( "ifAvailable" => true.into() )
        };

        // fn returns a promise that gets resolved when the callback returns,
        // which is useless for us. Actual lock is gets passed to the callback
        let _promise = lock_manager.request_with_options_and_callback(name, &opts, &cb);

        let would_block = would_block_rx.await.expect("valid singleshot channel");

        // Returning here also resolves the promise
        if would_block {
            return Err(Error::WouldBlock);
        }

        Ok(NamedLock {
            _unlock_guard: unlock_token_guard,
        })
    }
}

impl fmt::Debug for NamedLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NamedLock { .. }")
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, js_name = LockManager, typescript_type = "LockManager")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    /// The `LockManager` class.
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/LockManager)
    pub type LockManager;

    #[wasm_bindgen(method, structural, js_class = "LockManager", js_name = request)]
    /// The `request()` method.
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/LockManager/request)
    pub fn request_with_options_and_callback(
        this: &LockManager,
        name: &str,
        options: &JsValue,
        callback: &Function,
    ) -> Promise;
}

fn get_lock_manager() -> Result<LockManager, JsError> {
    const NAVIGATOR_PROPERTY: &str = "navigator";
    const LOCK_MANAGER_PROPERTY: &str = "locks";

    let navigator_key = to_value(NAVIGATOR_PROPERTY).expect("successful conversion");
    let lock_manager_key = to_value(LOCK_MANAGER_PROPERTY).expect("successful conversion");

    let scope = js_sys::global();
    let Ok(navigator) = Reflect::get(&scope, &navigator_key) else {
        return Err(JsError::new("`navigator` not found in global scope"));
    };
    match Reflect::get(&navigator, &lock_manager_key) {
        Ok(manager) => Ok(manager.unchecked_into::<LockManager>()),
        Err(_) => Err(JsError::new("`navigator.locks` not found in global scope")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    use lumina_utils::executor::spawn;
    use lumina_utils::time::sleep;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    async fn lock_unlock() {
        const LOCK_NAME: &str = "lock_unlock";
        {
            let _guard = NamedLock::try_lock(LOCK_NAME).await.expect("lock ok");
            NamedLock::try_lock(LOCK_NAME)
                .await
                .expect_err("locked lock");
        }

        // XXX: a bit nasty, but we need to yield back to js for unlock to register
        lumina_utils::executor::yield_now().await;

        let _guard = NamedLock::try_lock(LOCK_NAME).await.expect("valid lock");
    }

    #[wasm_bindgen_test]
    async fn blocking_lock_interop() {
        const LOCK_NAME: &str = "blocking_lock_interop";
        let lock = NamedLock::try_lock(LOCK_NAME).await.expect("lock ok");

        let (tx, rx) = oneshot::channel();
        spawn(async move {
            NamedLock::try_lock(LOCK_NAME)
                .await
                .expect_err("should be locked now");
            let _sync_lock = NamedLock::lock(LOCK_NAME).await.unwrap();
            rx.await.unwrap();
        });

        sleep(Duration::from_millis(100)).await;
        drop(lock);

        lumina_utils::executor::yield_now().await;
        NamedLock::try_lock(LOCK_NAME)
            .await
            .expect_err("should be locked");

        tx.send(()).unwrap();
        lumina_utils::executor::yield_now().await;
        NamedLock::try_lock(LOCK_NAME).await.expect("unlocked");
    }
}
