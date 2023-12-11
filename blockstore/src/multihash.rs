use cid::CidGeneric;
use multihash::Multihash;

use crate::Result;

pub trait HasMultihash<const S: usize> {
    fn multihash(&self) -> Result<Multihash<S>>;
}

pub trait HasCid<const S: usize>: HasMultihash<S> {
    fn cid_v1(&self) -> Result<CidGeneric<S>> {
        Ok(CidGeneric::<S>::new_v1(Self::codec(), self.multihash()?))
    }

    fn codec() -> u64;
}

pub trait Block<const S: usize>: HasCid<S> + AsRef<[u8]> + Sync + Send {}
impl<const S: usize, T: HasCid<S> + AsRef<[u8]> + Sync + Send> Block<S> for T {}
