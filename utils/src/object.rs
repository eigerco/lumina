#[macro_export]
/// Create a new javascript `Object` with given properties
#[cfg(all(target_arch = "wasm32", feature = "make-object"))]
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
