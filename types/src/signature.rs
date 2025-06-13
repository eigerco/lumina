#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use js_sys::Uint8Array;
    use tendermint::signature::Signature;
    use wasm_bindgen::prelude::*;

    #[derive(Clone, Debug)]
    #[wasm_bindgen(getter_with_clone, js_name = "Signature")]
    pub struct JsSignature(pub Uint8Array);

    impl From<Signature> for JsSignature {
        fn from(value: Signature) -> Self {
            JsSignature(Uint8Array::from(value.as_bytes()))
        }
    }

    impl From<&[u8]> for JsSignature {
        fn from(value: &[u8]) -> Self {
            JsSignature(Uint8Array::from(value))
        }
    }
}

#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use tendermint::signature::Signature as TendermintSignature;
    use uniffi::Record;

    use crate::UniffiConversionError;

    /// Signature
    #[derive(Record)]
    pub struct Signature {
        signature: Vec<u8>,
    }

    impl TryFrom<Signature> for TendermintSignature {
        type Error = UniffiConversionError;

        fn try_from(value: Signature) -> Result<Self, Self::Error> {
            TendermintSignature::new(&value.signature)
                .map_err(|_| UniffiConversionError::InvalidSignatureLength)?
                .ok_or(UniffiConversionError::InvalidSignatureLength)
        }
    }

    impl From<TendermintSignature> for Signature {
        fn from(value: TendermintSignature) -> Self {
            Signature {
                signature: value.into_bytes(),
            }
        }
    }

    uniffi::custom_type!(TendermintSignature, Signature, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into()
    });
}
