use cid::CidGeneric;
use multihash::Multihash;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum CidError {
    #[error("Invalid CID codec {0}")]
    InvalidCidCodec(u64),

    #[error("Invalid multihash length {0}")]
    InvalidMultihashLength(usize),

    #[error("Invalid multihash code {0} expected {1}")]
    InvalidMultihashCode(u64, u64),

    #[error("Invalid data format {0}")]
    InvalidDataFormat(String),

    #[error("Invalid CID: {0}")]
    InvalidCid(String),
}

pub trait HasMultihash<const S: usize> {
    fn multihash(&self) -> Result<Multihash<S>, CidError>;
}

pub trait HasCid<const S: usize>: HasMultihash<S> {
    fn cid_v1(&self) -> Result<CidGeneric<S>, CidError> {
        Ok(CidGeneric::<S>::new_v1(Self::codec(), self.multihash()?))
    }

    fn codec() -> u64;
}

pub trait Block<const S: usize>: HasCid<S> + AsRef<[u8]> + Sync + Send {}
impl<const S: usize, T: HasCid<S> + AsRef<[u8]> + Sync + Send> Block<S> for T {}
