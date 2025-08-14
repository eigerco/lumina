// TODO: upstream to `named_lock`

use std::fmt;

use js_sys::{Function, Object, Promise, Reflect};
use lumina_utils::make_object;
use serde_wasm_bindgen::to_value;
use tokio::sync::oneshot;
use wasm_bindgen::prelude::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Call would block")]
    WouldBlock,

    #[error("Could not acquire lock manager")]
    LockManagerUnavailable(JsError),
}

pub struct NamedLock {
    lock_manager: LockManager,
    name: String,
}

pub struct NamedLockGuard {
    resolve_fn: Function,
}

impl NamedLock {
    pub fn create(name: impl Into<String>) -> Result<Self, Error> {
        let lock_manager = get_lock_manager().map_err(Error::LockManagerUnavailable)?;

        Ok(NamedLock {
            lock_manager,
            name: name.into(),
        })
    }

    async fn lock_impl(&self, block: bool) -> Result<NamedLockGuard, Error> {
        // what follows is a rough translaction of mdn example into rust
        // https://developer.mozilla.org/en-US/docs/Web/API/Web_Locks_API#advanced_use
        let (promise, resolve_fn, _reject) = promise_with_resolvers();

        let (tx, rx) = oneshot::channel();

        let cb: Function = Closure::once_into_js(move |lock: JsValue| {
            let _ = tx.send(lock);
            promise
        })
        .unchecked_into();

        let opts = if block {
            Object::new()
        } else {
            make_object!( "ifAvailable" => true.into() )
        };

        // fn returns a promise that gets resolved when the callback returns,
        // which is useless for us. Actual lock is gets passed to the callback
        let _promise = self
            .lock_manager
            .request_with_options_and_callback(&self.name, &opts, &cb);

        let lock = rx.await.expect("valid singleshot channel");

        // only case where callback above is called with a falsy value is when we're in
        // non-blocking mode and the lock is already taken
        if !lock.is_truthy() {
            return Err(Error::WouldBlock);
        }

        Ok(NamedLockGuard { resolve_fn })
    }

    pub async fn lock(&self) -> Result<NamedLockGuard, Error> {
        self.lock_impl(true).await
    }

    pub async fn try_lock(&self) -> Result<NamedLockGuard, Error> {
        self.lock_impl(false).await
    }
}

impl Drop for NamedLockGuard {
    fn drop(&mut self) {
        let _ = self.resolve_fn.call0(&JsValue::NULL);
    }
}

impl fmt::Debug for NamedLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NamedLock({})", self.name)
    }
}

impl fmt::Debug for NamedLockGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NamedLockGuard { .. }")
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, js_name = LockManager, typescript_type = "LockManager")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    /// The `LockManager` class.
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/LockManager)
    pub type LockManager;

    #[wasm_bindgen(method, structural, js_class = "LockManager", js_name = query)]
    /// The `query()` method.
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/LockManager/query)
    pub fn query(this: &LockManager) -> Promise;

    #[wasm_bindgen(method, structural, js_class = "LockManager", js_name = request)]
    /// The `request()` method.
    /// [MDN Documentation](https://developer.mozilla.org/en-US/docs/Web/API/LockManager/request)
    pub fn request_with_callback(this: &LockManager, name: &str, callback: &Function) -> Promise;

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

fn promise_with_resolvers() -> (Promise, Function, Function) {
    let a = js::Promise::with_resolvers();

    let promise = Reflect::get(&a, &to_value("promise").unwrap())
        .unwrap()
        .dyn_into::<Promise>()
        .expect("valid cast");

    let resolve = Reflect::get(&a, &to_value("resolve").unwrap())
        .unwrap()
        .dyn_into::<Function>()
        .expect("valid cast");

    let reject = Reflect::get(&a, &to_value("reject").unwrap())
        .unwrap()
        .dyn_into::<Function>()
        .expect("valid cast");

    (promise, resolve, reject)
}

mod js {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        /// Js Promise
        pub type Promise;

        /// The Promise.withResolvers() static method returns an object containing a new
        /// Promise object and two functions to resolve or reject it, corresponding to the
        /// two parameters passed to the executor of the Promise() constructor.
        ///
        /// https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Promise/withResolvers
        #[wasm_bindgen(static_method_of = Promise, js_name = "withResolvers")]
        pub fn with_resolvers() -> JsValue;
    }
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
    use super::*;

    use lumina_utils::time::sleep;
    use std::time::Duration;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    async fn lock_unlock() {
        let lock = NamedLock::create("foo");

        {
            let _guard = lock.try_lock().await.expect("valid lock");
            let _locked_guard = lock.try_lock().await.expect_err("locked lock");
        }

        // XXX: a bit nasty, but we need to yield back to js for it to register the unlock
        sleep(Duration::from_millis(1)).await;

        let _guard = lock.try_lock().await.expect("valid lock");
    }
}
