/// Create a new javascript `Object` with given properties
#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
macro_rules! make_object {
    ($( $prop:expr => $val:expr ),+) => {{
        let object = ::js_sys::Object::new();
        $(
            ::js_sys::Reflect::set(
                &object,
                &$prop.into(),
                &$val,
            )
            .expect("setting field on new object");
        )+
        object
    }};
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub(crate) use make_object;
