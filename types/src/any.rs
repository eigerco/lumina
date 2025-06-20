//! Helpers for dealing with `Any` types conversion

use prost::Name;
use tendermint_proto::google::protobuf::Any;

/// Value convertion into protobuf's Any
pub trait IntoAny {
    /// Converts itself into protobuf's Any type
    fn into_any(self) -> Any;
}

impl<T> IntoAny for T
where
    T: Name,
{
    fn into_any(self) -> Any {
        Any {
            type_url: T::type_url(),
            value: self.encode_to_vec(),
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use super::*;
    use js_sys::Uint8Array;
    use lumina_utils::make_object;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(typescript_custom_section)]
    const _: &str = "
    /**
     * Protobuf Any type
     */
    export interface ProtoAny {
      typeUrl: string;
      value: Uint8Array;
    }
    ";

    #[wasm_bindgen]
    extern "C" {
        /// Protobuf Any exposed to javascript
        #[wasm_bindgen(typescript_type = "ProtoAny")]
        pub type JsAny;

        /// Any type URL
        #[wasm_bindgen(method, getter, js_name = typeUrl)]
        pub fn type_url(this: &JsAny) -> String;

        /// Any type value
        #[wasm_bindgen(method, getter, js_name = value)]
        pub fn value(this: &JsAny) -> Vec<u8>;
    }

    impl From<Any> for JsAny {
        fn from(value: Any) -> Self {
            let obj = make_object!(
                "typeUrl" => value.type_url.into(),
                "value" => Uint8Array::from(&value.value[..])
            );

            obj.unchecked_into()
        }
    }

    impl IntoAny for JsAny {
        fn into_any(self) -> Any {
            Any {
                type_url: self.type_url(),
                value: self.value(),
            }
        }
    }
}

/// uniffi conversion types
#[cfg(feature = "uniffi")]
mod uniffi_types {
    use tendermint_proto::google::protobuf::Any as ProtobufAny;

    /// Any contains an arbitrary serialized protocol buffer message along with a URL that
    /// describes the type of the serialized message.
    #[uniffi::remote(Record)]
    pub struct ProtobufAny {
        /// A URL/resource name that uniquely identifies the type of the serialized protocol
        /// buffer message. This string must contain at least one “/” character. The last
        /// segment of the URL’s path must represent the fully qualified name of the type
        /// (as in path/google.protobuf.Duration). The name should be in a canonical form
        /// (e.g., leading “.” is not accepted).
        pub type_url: String,
        /// Must be a valid serialized protocol buffer of the above specified type.
        pub value: Vec<u8>,
    }
}
