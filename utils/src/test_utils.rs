#[cfg(not(target_arch = "wasm32"))]
pub(crate) use tokio::test as async_test;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm_bindgen_test::wasm_bindgen_test as async_test;
