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

/// uniffi conversion types
#[cfg(feature = "uniffi")]
pub mod uniffi_types {
    use super::Hash as TendermintHash;
    use tendermint::hash::Algorithm;

    use uniffi::Enum;

    use crate::UniffiError;

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
        type Error = UniffiError;

        fn try_from(value: UniffiHash) -> Result<Self, Self::Error> {
            Ok(match value {
                UniffiHash::Sha256 { hash } => TendermintHash::from_bytes(Algorithm::Sha256, &hash)
                    .map_err(|_| UniffiError::InvalidHashLength)?,
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
}
