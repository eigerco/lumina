use js_sys::Uint8Array;
use lumina_utils::make_object;
use prost::Name;
use tendermint_proto::google::protobuf::Any;
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

    #[wasm_bindgen(method, getter, js_name = typeUrl)]
    pub fn type_url(this: &JsAny) -> String;

    #[wasm_bindgen(method, getter, js_name = value)]
    pub fn value(this: &JsAny) -> Vec<u8>;
}

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

impl IntoAny for JsAny {
    fn into_any(self) -> Any {
        Any {
            type_url: self.type_url(),
            value: self.value(),
        }
    }
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
