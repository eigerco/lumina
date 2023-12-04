//! Celestia hash related types and traits.

/// The hash type used commonly in the Celestia.
pub type Hash = tendermint::hash::Hash;

/// A trait extending the [`Hash`] functionalities.
///
/// [`Hash`]: crate::hash::Hash
pub trait HashExt {
    /// Get the default `SHA256` hash.
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
