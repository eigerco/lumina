use std::fmt::{self, Debug, Display};
use std::ops::RangeBounds;

use async_trait::async_trait;
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use cid::Cid;

use crate::store::{BlockRanges, Result, SamplingMetadata, Store, VerifiedExtendedHeaders};

/// Struct that can be used to build combinations of different [`Store`] types.
///
/// # Example
///
/// ```ignore
/// type SuperStore = EitherStore<InMemoryStore, RedbStore>;
/// ```
pub enum EitherStore<L, R>
where
    L: Store,
    R: Store,
{
    /// A value of type `L`.
    Left(L),
    /// A value of type `R`.
    Right(R),
}

impl<L, R> EitherStore<L, R>
where
    L: Store,
    R: Store,
{
    /// Returns true if value is the `Left` variant.
    pub fn is_left(&self) -> bool {
        match self {
            EitherStore::Left(_) => true,
            EitherStore::Right(_) => false,
        }
    }

    /// Returns true if value is the `Right` variant.
    pub fn is_right(&self) -> bool {
        match self {
            EitherStore::Left(_) => false,
            EitherStore::Right(_) => true,
        }
    }

    /// Returns a reference of the left side of `EitherStore<L, R>`.
    pub fn left(&self) -> Option<&L> {
        match self {
            EitherStore::Left(store) => Some(store),
            EitherStore::Right(_) => None,
        }
    }

    /// Returns a reference of the right side of `EitherStore<L, R>`.
    pub fn right(&self) -> Option<&R> {
        match self {
            EitherStore::Left(_) => None,
            EitherStore::Right(store) => Some(store),
        }
    }

    /// Returns a mutable reference of the left side of `EitherStore<L, R>`.
    pub fn left_mut(&mut self) -> Option<&mut L> {
        match self {
            EitherStore::Left(store) => Some(store),
            EitherStore::Right(_) => None,
        }
    }

    /// Returns a mutable reference of the right side of `EitherStore<L, R>`.
    pub fn right_mut(&mut self) -> Option<&mut R> {
        match self {
            EitherStore::Left(_) => None,
            EitherStore::Right(store) => Some(store),
        }
    }

    /// Returns the left side of `EitherStore<L, R>`.
    pub fn into_left(self) -> Option<L> {
        match self {
            EitherStore::Left(store) => Some(store),
            EitherStore::Right(_) => None,
        }
    }

    /// Returns the right side of `EitherStore<L, R>`.
    pub fn into_right(self) -> Option<R> {
        match self {
            EitherStore::Left(_) => None,
            EitherStore::Right(store) => Some(store),
        }
    }
}

impl<L, R> Clone for EitherStore<L, R>
where
    L: Store + Clone,
    R: Store + Clone,
{
    fn clone(&self) -> Self {
        match self {
            EitherStore::Left(store) => EitherStore::Left(store.clone()),
            EitherStore::Right(store) => EitherStore::Right(store.clone()),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        match source {
            EitherStore::Left(source_store) => match self {
                EitherStore::Left(ref mut self_store) => self_store.clone_from(source_store),
                EitherStore::Right(_) => *self = EitherStore::Left(source_store.clone()),
            },
            EitherStore::Right(source_store) => match self {
                EitherStore::Left(_) => *self = EitherStore::Right(source_store.clone()),
                EitherStore::Right(ref mut self_store) => self_store.clone_from(source_store),
            },
        };
    }
}

impl<L, R> Debug for EitherStore<L, R>
where
    L: Store + Debug,
    R: Store + Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EitherStore::Left(ref store) => Debug::fmt(store, f),
            EitherStore::Right(ref store) => Debug::fmt(store, f),
        }
    }
}

impl<L, R> Display for EitherStore<L, R>
where
    L: Store + Display,
    R: Store + Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EitherStore::Left(ref store) => Display::fmt(store, f),
            EitherStore::Right(ref store) => Display::fmt(store, f),
        }
    }
}

macro_rules! call {
    ($self:ident, $method:ident($($param:expr),*)) =>  {
        match $self {
            EitherStore::Left(store) => store.$method($($param),*).await,
            EitherStore::Right(store) => store.$method($($param),*).await,
        }
    };
}

#[async_trait]
impl<L, R> Store for EitherStore<L, R>
where
    L: Store,
    R: Store,
{
    async fn get_head(&self) -> Result<ExtendedHeader> {
        call!(self, get_head())
    }

    async fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        call!(self, get_by_hash(hash))
    }

    async fn get_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        call!(self, get_by_height(height))
    }

    async fn wait_new_head(&self) -> u64 {
        call!(self, wait_new_head())
    }

    async fn wait_height(&self, height: u64) -> Result<()> {
        call!(self, wait_height(height))
    }

    async fn get_range<RB>(&self, range: RB) -> Result<Vec<ExtendedHeader>>
    where
        RB: RangeBounds<u64> + Send,
    {
        call!(self, get_range(range))
    }

    async fn head_height(&self) -> Result<u64> {
        call!(self, head_height())
    }

    async fn has(&self, hash: &Hash) -> bool {
        call!(self, has(hash))
    }

    async fn has_at(&self, height: u64) -> bool {
        call!(self, has_at(height))
    }

    async fn update_sampling_metadata(&self, height: u64, cids: Vec<Cid>) -> Result<()> {
        call!(self, update_sampling_metadata(height, cids))
    }

    async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        call!(self, get_sampling_metadata(height))
    }

    async fn mark_sampled(&self, height: u64) -> Result<()> {
        call!(self, mark_sampled(height))
    }

    async fn insert<H>(&self, headers: H) -> Result<()>
    where
        H: TryInto<VerifiedExtendedHeaders> + Send,
        <H as TryInto<VerifiedExtendedHeaders>>::Error: Display,
    {
        call!(self, insert(headers))
    }

    async fn get_stored_header_ranges(&self) -> Result<BlockRanges> {
        call!(self, get_stored_header_ranges())
    }

    async fn get_sampled_ranges(&self) -> Result<BlockRanges> {
        call!(self, get_sampled_ranges())
    }

    async fn get_pruned_ranges(&self) -> Result<BlockRanges> {
        call!(self, get_pruned_ranges())
    }

    async fn remove_height(&self, height: u64) -> Result<()> {
        call!(self, remove_height(height))
    }

    async fn close(self) -> Result<()> {
        call!(self, close())
    }
}
