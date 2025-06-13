//! Celestia hash related types and traits.

/// The hash type used commonly in the Celestia.
pub type Hash = tendermint::hash::Hash;

/// A trait extending [`Hash`] functionality.
///
/// [`Hash`]: crate::hash::Hash
pub trait HashExt {
    /// Get the `SHA256` hash of an empty input.
    ///
    /// This is equivalent to `sha256sum /dev/null`.
    fn default_sha256() -> Hash;
}

impl HashExt for Hash {
    fn default_sha256() -> Hash {
        Hash::Sha256([
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ])
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use super::Hash;
    use js_sys::Uint8Array;
    use tendermint::hash::{Algorithm, SHA256_HASH_SIZE};

    /// Hash digest
    pub struct JsHash {
        /// Sha256 or empty hash
        hash: Option<Uint8Array>,
    }

    impl From<Hash> for JsHash {
        fn from(value: Hash) -> Self {
            match value {
                Hash::None => JsHash { hash: None },
                Hash::Sha256(digest) => {
                    let array = Uint8Array::new_with_length(
                        SHA256_HASH_SIZE.try_into().expect("valid hash size"),
                    );
                    array.copy_from(&digest);
                    JsHash { hash: Some(array) }
                }
            }
        }
    }

    impl TryFrom<JsHash> for Hash {
        type Error = tendermint::Error;

        fn try_from(value: JsHash) -> Result<Self, Self::Error> {
            let Some(digest) = value.hash else {
                return Ok(Hash::None);
            };

            Hash::from_bytes(Algorithm::Sha256, &digest.to_vec())
        }
    }
}

/// uniffi conversion types
#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use super::Hash as TendermintHash;
    use tendermint::hash::Algorithm;
    use tendermint::hash::AppHash as TendermintAppHash;

    use uniffi::{Enum, Record};

    use crate::error::UniffiConversionError;

    /// Hash digest
    #[derive(Enum)]
    pub enum UniffiHash {
        /// SHA-256 hash
        Sha256 {
            /// hash value
            hash: Vec<u8>,
        },
        /// Empty hash
        None,
    }

    impl TryFrom<UniffiHash> for TendermintHash {
        type Error = UniffiConversionError;

        fn try_from(value: UniffiHash) -> Result<Self, Self::Error> {
            Ok(match value {
                UniffiHash::Sha256 { hash } => TendermintHash::from_bytes(Algorithm::Sha256, &hash)
                    .map_err(|_| UniffiConversionError::InvalidHashLength)?,
                UniffiHash::None => TendermintHash::None,
            })
        }
    }

    impl From<TendermintHash> for UniffiHash {
        fn from(value: TendermintHash) -> Self {
            match value {
                TendermintHash::Sha256(hash) => UniffiHash::Sha256 {
                    hash: hash.to_vec(),
                },
                TendermintHash::None => UniffiHash::None,
            }
        }
    }

    uniffi::custom_type!(TendermintHash, UniffiHash, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into()
    });

    /// AppHash is usually a SHA256 hash, but in reality it can be any kind of data
    #[derive(Record)]
    pub struct AppHash {
        /// AppHash value
        pub hash: Vec<u8>,
    }

    impl TryFrom<AppHash> for TendermintAppHash {
        type Error = UniffiConversionError;

        fn try_from(value: AppHash) -> Result<Self, Self::Error> {
            Ok(TendermintAppHash::try_from(value.hash).expect("conversion to be infallible"))
        }
    }

    impl From<TendermintAppHash> for AppHash {
        fn from(value: TendermintAppHash) -> Self {
            AppHash {
                hash: value.as_bytes().to_vec(),
            }
        }
    }

    uniffi::custom_type!(TendermintAppHash, AppHash, {
        remote,
        try_lift: |value| Ok(value.try_into()?),
        lower: |value| value.into(),
    });
}
