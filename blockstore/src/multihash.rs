use cid::CidGeneric;
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

/// Trait for a block of data which can be represented as a string of bytes, which can compute its
/// own CID
pub trait Block<const S: usize>: Sync + Send {
    fn cid(&self) -> Result<CidGeneric<S>, CidError>;
    fn data(&self) -> &[u8];
}
