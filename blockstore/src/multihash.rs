use cid::CidGeneric;
use multihash::Multihash;
use thiserror::Error;

/// Error returned when trying to compute new or parse existing CID. Note that errors here can be
/// specific to particular [`Block`] impl, and don't necessarily indicate invalid CID in general.
#[derive(Debug, Error, PartialEq)]
pub enum CidError {
    /// Coded specified in CID is not supported in this context
    #[error("Invalid CID codec {0}")]
    InvalidCidCodec(u64),

    /// CID's multihash length is different from expected
    #[error("Invalid multihash length {0}")]
    InvalidMultihashLength(usize),

    /// Encountered multihash code is not supported in this context
    #[error("Invalid multihash code {0} expected {1}")]
    InvalidMultihashCode(u64, u64),

    /// CID could not be computed for the provided data due to some constraint violation
    #[error("Invalid data format {0}")]
    InvalidDataFormat(String),

    /// CID is well-formed but contains invalid data
    #[error("Invalid CID: {0}")]
    InvalidCid(String),
}

/// Trait indicating a struct that can compute its own multihash
pub trait HasMultihash<const S: usize> {
    fn multihash(&self) -> Result<Multihash<S>, CidError>;
}

/// Trait indicating a struct that can compute its own CID
pub trait HasCid<const S: usize>: HasMultihash<S> {
    fn cid_v1(&self) -> Result<CidGeneric<S>, CidError> {
        Ok(CidGeneric::<S>::new_v1(Self::codec(), self.multihash()?))
    }

    fn codec() -> u64;
}

/// Trait for structs that can be inserted into the [`Blockstore`]. For this purpose they need to
/// know how to compute their own CID ([`HasCid`] trait) as well as have a byte string
/// representation (`AsRef[u8]` trait). If CID is known, or it's otherwise more conventient, one
/// can use [`Blockstore::put_keyed`] with `CID` and `&[u8]` directly.
///
/// [`Blockstore`]: crate::Blockstore
/// [`Blockstore::put_keyed`]: crate::Blockstore::put_keyed
pub trait Block<const S: usize>: HasCid<S> + AsRef<[u8]> + Sync + Send {}

impl<const S: usize, T: HasCid<S> + AsRef<[u8]> + Sync + Send> Block<S> for T {}
