//! Pool tracking for ShrEx protocol
//!
//! This module implements pool tracking with hash-specific pools and validation-based promotion.
//! When peers announce data availability via ShrEx/Sub, they are added to hash-specific pools.
//! Once a header is received and validated, the pool is promoted and peers are added to the
//! general discovered peers pool.

use std::collections::{HashMap, HashSet};
use std::slice::Iter;
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use celestia_proto::share::p2p::shrex::sub::RecentEdsNotification;
use celestia_types::{ExtendedHeader, hash::Hash};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use libp2p::PeerId;
use lumina_utils::time::{Elapsed, timeout};
use prost::Message;
use tracing::{debug, trace, warn};

use crate::p2p::shrex::Event;
use crate::store::{Store, StoreError};

const ROOT_HASH_WINDOW: u64 = 10;
const POOL_VALIDATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Pool tracker for managing hash-specific and discovered peer pools
pub struct PoolTracker<S> {
    /// Height-specific pools: height -> peer pool
    hash_pools: HashMap<u64, PeerPool>,

    validated_pools: HashMap<Hash, Vec<PeerId>>,
    /// Highest known validated store height
    subjective_head: Option<u64>,
    /// Header store
    store: Arc<S>,

    new_headers_tasks:
        FuturesUnordered<BoxFuture<'static, Result<ExtendedHeader, HeaderTaskError>>>,
}

#[derive(Debug, Clone, PartialEq)]
enum PeerPool {
    /// Candidates: height hasn't been validated yet, stores potential matches by hash
    Candidates((HashSet<PeerId>, HashMap<Hash, Vec<PeerId>>)),
    /// Validated: hash for this height has been accepted, all peers moved to discovered
    Validated(Hash),
}

impl Default for PeerPool {
    fn default() -> Self {
        PeerPool::Candidates((HashSet::new(), HashMap::new()))
    }
}

pub(super) struct EdsNotification {
    /// Block height this EDS belongs to
    pub height: u64,
    /// Data hash
    pub data_hash: Hash,
}

#[derive(thiserror::Error, Debug)]
pub enum NotifcationValidationError {
    /// Error deserializing received notification
    #[error("Error deserializing message: {0}")]
    ErrorDeserializingMessage(#[from] prost::DecodeError),
    /// Invalid notification with wrong hash length
    #[error("Invalid hash length, expected 32, got {0}")]
    InvalidHashLength(usize),
    /// Invalid notification with all-zero hash
    #[error("Received notification with zero hash")]
    InvalidZeroHash,
    /// Invalid notification with zero height
    #[error("Received notification with zero height")]
    InvalidZeroHeight,
}

#[derive(thiserror::Error, Debug)]
enum HeaderTaskError {
    /// Timeout occurred before header was received and validated by the node
    #[error("Timeout waiting for header at height {0}")]
    Timeout(u64),
    /// Propagated from the [`Store`]
    #[error("Store error when waiting for header at {height}: {source}")]
    StoreError { height: u64, source: StoreError },
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum GetPoolError {
    #[error("Pool candidates exist but aren't validated yet")]
    CandidatesNotValidated,

    #[error("Pool for given height is old and was likely already pruned")]
    HeightTooOld,

    #[error("Height not tracked, either from future, or no notifications received")]
    HeightNotTracked,
}

impl EdsNotification {
    pub fn deserialize_and_validate(data: &[u8]) -> Result<Self, NotifcationValidationError> {
        let RecentEdsNotification { height, data_hash } = RecentEdsNotification::decode(data)?;
        if height == 0 {
            return Err(NotifcationValidationError::InvalidZeroHeight);
        }
        if data_hash.iter().all(|v| *v == 0) {
            return Err(NotifcationValidationError::InvalidZeroHash);
        }
        let data_hash = Hash::Sha256(
            data_hash
                .try_into()
                .map_err(|v: Vec<_>| NotifcationValidationError::InvalidHashLength(v.len()))?,
        );
        Ok(EdsNotification { height, data_hash })
    }
}

impl<S> PoolTracker<S>
where
    S: Store + 'static,
{
    /// Create a new PoolTracker with custom stored heights amount
    pub fn new(store: Arc<S>) -> Self {
        let s = store.clone();
        let get_subjective_head = async move {
            let header = match s.get_head().await {
                Err(StoreError::NotFound) => {
                    // empty store returns NotFound for head, wait for first header
                    s.wait_new_head().await;
                    s.get_head().await
                }
                other => other,
            }
            // Height is used only for cleaning up pools so it's ok to pass dummy value
            // here as we don't have any yet.
            .map_err(|source| HeaderTaskError::StoreError { height: 0, source })?;
            Ok(header)
        }
        .boxed();

        Self {
            hash_pools: HashMap::new(),
            validated_pools: HashMap::new(),
            subjective_head: None,
            store,
            new_headers_tasks: FuturesUnordered::from_iter([get_subjective_head]),
        }
    }

    /// Add peer from ShrEx/Sub notification
    ///
    /// Called when receiving a ShrEx/Sub notification. The peer is added to the hash-specific pool.
    /// If the pool is already validated, the peer is immediately promoted to discovered peers.
    pub fn add_peer_for_hash(
        &mut self,
        peer_id: PeerId,
        data_hash: Hash,
        height: u64,
    ) -> Option<Event> {
        if self
            .subjective_head
            .map(stale_height_threshold)
            .is_none_or(|stale_height| height <= stale_height)
        {
            return None;
        }

        let pool = match self.hash_pools.get_mut(&height) {
            Some(pool) => pool,
            None => {
                trace!("New pool for height {height}");
                self.queue_get_header_from_store(height);
                self.hash_pools.entry(height).or_default()
            }
        };

        match pool {
            PeerPool::Candidates((voted, candidates)) => {
                if !voted.insert(peer_id) {
                    // duplicate vote
                    return Some(Event::PoolUpdate {
                        blacklist_peers: vec![peer_id],
                        add_peers: vec![],
                    });
                }
                candidates
                    .entry(data_hash)
                    .or_insert_with(Vec::new)
                    .push(peer_id);
                None
            }
            PeerPool::Validated(validated_hash) => {
                if *validated_hash == data_hash {
                    self.validated_pools
                        .entry(data_hash)
                        .or_default()
                        .push(peer_id);
                    Some(Event::PoolUpdate {
                        blacklist_peers: vec![],
                        add_peers: vec![peer_id],
                    })
                } else {
                    Some(Event::PoolUpdate {
                        blacklist_peers: vec![peer_id],
                        add_peers: vec![],
                    })
                }
            }
        }
    }

    /// Return peer pool for the provided block height.
    ///
    /// Returns a list of peers that can be queried for data at a given block.
    /// If the pool doesn't exist, it might already be pruned, not yet exist, or
    /// tracker hasn't received any notifications about availability, e.g. if the
    /// block was empty.
    ///
    /// In case the pool exists but has no peers, we only received invalid notifications,
    /// i.e. all the peers advertised data root that didn't match the one from the synced
    /// header.
    pub fn get_pool(&self, height: u64) -> Result<Iter<'_, PeerId>, GetPoolError> {
        match self.hash_pools.get(&height) {
            Some(PeerPool::Validated(data_hash)) => Ok(self
                .validated_pools
                .get(data_hash)
                .expect("must exist if hash_pool exists")
                .iter()),
            Some(PeerPool::Candidates(_)) => Err(GetPoolError::CandidatesNotValidated),
            None => {
                if self
                    .subjective_head
                    .is_some_and(|head| height <= stale_height_threshold(head))
                {
                    Err(GetPoolError::HeightTooOld)
                } else {
                    Err(GetPoolError::HeightNotTracked)
                }
            }
        }
    }

    /// Remove peer from all pools
    #[allow(dead_code)] // TODO: remove once integrated
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        for pool in self.hash_pools.values_mut() {
            match pool {
                PeerPool::Candidates((voted, candidates)) => {
                    voted.remove(peer_id);
                    for peers in candidates.values_mut() {
                        peers.retain(|p| p != peer_id);
                    }
                }
                PeerPool::Validated(_) => (),
            }
        }

        for pool in self.validated_pools.values_mut() {
            pool.retain(|p| p != peer_id);
        }
    }

    fn queue_get_header_from_store(&mut self, height: u64) {
        let store = self.store.clone();

        self.new_headers_tasks.push(
            async move {
                timeout(POOL_VALIDATION_TIMEOUT, store.wait_height(height))
                    .await
                    .map_err(|_: Elapsed| HeaderTaskError::Timeout(height))?
                    .map_err(|source| HeaderTaskError::StoreError { height, source })?;
                store
                    .get_by_height(height)
                    .await
                    .map_err(|source| HeaderTaskError::StoreError { height, source })
            }
            .boxed(),
        );
    }

    pub(super) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        loop {
            let Some(header) = ready!(self.new_headers_tasks.poll_next_unpin(cx)) else {
                return Poll::Pending;
            };

            let header = match header {
                Ok(h) => h,
                Err(HeaderTaskError::Timeout(height)) => {
                    self.hash_pools.remove(&height);
                    continue;
                }
                Err(HeaderTaskError::StoreError { height, source }) => {
                    debug!("Store error waiting for header at {height}: {source}");
                    self.hash_pools.remove(&height);
                    continue;
                }
            };

            let height = header.height();
            let data_hash = header
                .header
                .data_hash
                .expect("headers from store must pass validate");

            self.try_update_subjective_head(height);

            return Poll::Ready(self.validate_pool(data_hash, height));
        }
    }

    /// Validate pool on header receipt
    ///
    /// Validates the pool with a valid header, promoting all peers for the matching hash to
    /// discovered peers.
    fn validate_pool(&mut self, data_hash: Hash, height: u64) -> Option<Event> {
        if let Some(pool) = self.hash_pools.get_mut(&height) {
            match pool {
                PeerPool::Candidates((_, candidates)) => {
                    let validated_peers = candidates.remove(&data_hash).unwrap_or_default();

                    let wrong_peers: Vec<PeerId> = candidates
                        .values()
                        .flat_map(|pool| pool.iter().cloned())
                        .collect();

                    trace!(
                        "Promoted valid pool for {height} with {} peers, {} peers blacklisted",
                        validated_peers.len(),
                        wrong_peers.len()
                    );

                    self.validated_pools
                        .insert(data_hash, validated_peers.clone());
                    *pool = PeerPool::Validated(data_hash);

                    return Some(Event::PoolUpdate {
                        add_peers: validated_peers,
                        blacklist_peers: wrong_peers,
                    });
                }
                PeerPool::Validated(_) => {
                    warn!("Multiple validate_pool for the same height, should not happen");
                }
            }
        }
        None
    }

    fn try_update_subjective_head(&mut self, height: u64) {
        let Some(old_subjective_head) = self.subjective_head else {
            self.subjective_head = Some(height);
            return;
        };

        if height <= old_subjective_head {
            return;
        }

        let to_evict_start = stale_height_threshold(old_subjective_head);
        let to_evict_end = stale_height_threshold(height);
        self.subjective_head = Some(height);

        for h in to_evict_start..=to_evict_end {
            match self.hash_pools.remove(&h) {
                Some(PeerPool::Validated(hash)) => {
                    self.validated_pools.remove(&hash);
                }
                Some(PeerPool::Candidates(..)) | None => (),
            }
        }
    }
}

fn stale_height_threshold(subjective_head: u64) -> u64 {
    subjective_head.saturating_sub(ROOT_HASH_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryStore;
    use crate::test_utils::gen_filled_store;
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use futures::future::poll_fn;
    use lumina_utils::test_utils::async_test;

    fn vec_to_set<T: std::hash::Hash + Eq>(v: Vec<T>) -> HashSet<T> {
        v.into_iter().collect()
    }

    #[async_test]
    async fn notification_first() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();
        let height = header.height();
        let peer0 = PeerId::random();
        let hash0 = header.header.data_hash.unwrap();

        tracker.add_peer_for_hash(peer0, hash0, height);

        store.insert(header).await.unwrap();
        assert_peer_update(
            &poll_fn(|ctx| tracker.poll(ctx)).await.unwrap(),
            [&peer0],
            [],
        );

        let height_peers: Vec<_> = tracker.get_pool(height).unwrap().collect();
        assert_eq!(height_peers, vec![&peer0]);
    }

    #[async_test]
    async fn unknown_hash() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();
        let height = header.height();
        let peer0 = PeerId::random();
        let other_hash = Hash::Sha256([1u8; 32]);

        tracker.add_peer_for_hash(peer0, other_hash, height);

        store.insert(header).await.unwrap();
        assert_peer_update(
            &poll_fn(|ctx| tracker.poll(ctx)).await.unwrap(),
            [],
            [&peer0],
        );

        assert!(matches!(
            tracker.get_pool(height + 1),
            Err(GetPoolError::HeightNotTracked)
        ));
        assert!(tracker.get_pool(height).unwrap().count() == 0);
    }

    #[async_test]
    async fn pool_not_yet_validated() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();
        let height = header.height();
        let hash = header.header.data_hash.unwrap();
        let peer0 = PeerId::random();

        tracker.add_peer_for_hash(peer0, hash, height);

        assert!(matches!(
            tracker.get_pool(height),
            Err(GetPoolError::CandidatesNotValidated)
        ));

        store.insert(header).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let peers: Vec<_> = tracker.get_pool(height).unwrap().collect();
        assert_eq!(peers, vec![&peer0]);
    }

    #[async_test]
    async fn hash_selection() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();
        let height = header.height();

        let peer0 = PeerId::random();
        let peer1_0 = PeerId::random();
        let peer1_1 = PeerId::random();
        let hash0 = Hash::Sha256([1u8; 32]);
        let valid_hash = header.header.data_hash.unwrap();

        tracker.add_peer_for_hash(peer0, hash0, height);
        tracker.add_peer_for_hash(peer1_0, valid_hash, height);
        tracker.add_peer_for_hash(peer1_1, valid_hash, height);

        store.insert(header).await.unwrap();

        assert_peer_update(
            &poll_fn(|ctx| tracker.poll(ctx)).await.unwrap(),
            [&peer1_0, &peer1_1],
            [&peer0],
        );

        let height_peers: HashSet<_> = tracker.get_pool(height).unwrap().collect();
        assert_eq!(height_peers, vec_to_set(vec![&peer1_0, &peer1_1]));
    }

    #[async_test]
    async fn add_to_validated_pool() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();
        let height = header.height();

        let peer0 = PeerId::random();
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let peer3 = PeerId::random();
        let valid_hash = header.header.data_hash.unwrap();
        let invalid_hash = Hash::Sha256([2u8; 32]);

        tracker.add_peer_for_hash(peer0, valid_hash, height);

        store.insert(header).await.unwrap();
        assert_peer_update(
            &poll_fn(|ctx| tracker.poll(ctx)).await.unwrap(),
            [&peer0],
            [],
        );

        let peers: Vec<_> = tracker.get_pool(height).unwrap().collect();
        assert_eq!(peers, vec![&peer0]);

        assert_peer_update(
            &tracker
                .add_peer_for_hash(peer1, valid_hash, height)
                .unwrap(),
            [&peer1],
            [],
        );
        assert_peer_update(
            &tracker
                .add_peer_for_hash(peer2, valid_hash, height)
                .unwrap(),
            [&peer2],
            [],
        );
        assert_peer_update(
            &tracker
                .add_peer_for_hash(peer3, invalid_hash, height)
                .unwrap(),
            [],
            [&peer3],
        );

        let discovered_peers: HashSet<_> = tracker.get_pool(height).unwrap().collect();
        assert_eq!(discovered_peers, vec_to_set(vec![&peer0, &peer1, &peer2]));
    }

    #[async_test]
    async fn duplicate_votes() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();
        let height = header.height();

        let valid_hash = header.header.data_hash.unwrap();
        let peer0 = PeerId::random();
        let invalid_hash0 = Hash::Sha256([2u8; 32]);
        let peer1 = PeerId::random();
        let invalid_hash1 = Hash::Sha256([3u8; 32]);

        // only first vote should be counted
        tracker.add_peer_for_hash(peer0, valid_hash, height);
        tracker.add_peer_for_hash(peer0, invalid_hash0, height);

        tracker.add_peer_for_hash(peer1, invalid_hash1, height);
        tracker.add_peer_for_hash(peer1, valid_hash, height);

        store.insert(header).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let discovered_peers: Vec<_> = tracker.get_pool(height).unwrap().collect();
        assert_eq!(discovered_peers, vec![&peer0]);
    }

    #[async_test]
    async fn ignore_old_heights() {
        let (mut tracker, store, mut g) = setup_tracker(1).await;
        let old_header = g.next();
        let headers = g.next_many(ROOT_HASH_WINDOW);

        let valid_stale_hash = old_header.header.data_hash.unwrap();
        let stale_height = old_header.height();
        let old_peer = PeerId::random();

        store.insert(old_header).await.unwrap();

        let newest_hash = headers.last().unwrap().header.data_hash.unwrap();
        let newest_height = headers.last().unwrap().height();
        store.insert(headers).await.unwrap();

        // trigger subjective head update in peer tracker
        tracker.add_peer_for_hash(PeerId::random(), newest_hash, newest_height);
        poll_fn(|ctx| tracker.poll(ctx)).await;

        // receive notification about stale height
        tracker.add_peer_for_hash(old_peer, valid_stale_hash, stale_height);

        assert!(matches!(
            tracker.get_pool(stale_height),
            Err(GetPoolError::HeightTooOld)
        ));
    }

    #[async_test]
    async fn eviction() {
        let (mut tracker, store, mut g) = setup_tracker(3).await;
        let old_header = g.next();
        let headers = g.next_many(ROOT_HASH_WINDOW);
        let new_head = headers.last().unwrap().clone();

        let stale_hash = old_header.header.data_hash.unwrap();
        let stale_height = old_header.height();
        let old_peer = PeerId::random();

        tracker.add_peer_for_hash(old_peer, stale_hash, stale_height);
        store.insert(old_header).await.unwrap();
        assert_peer_update(
            &poll_fn(|ctx| tracker.poll(ctx)).await.unwrap(),
            [&old_peer],
            [],
        );

        let discovered_peers: Vec<_> = tracker.get_pool(stale_height).unwrap().collect();
        assert_eq!(discovered_peers, vec![&old_peer]);

        store.insert(headers).await.unwrap();
        let peer = PeerId::random();
        // Only signal actually moving the pool tracker forward in terms of new heights
        // is shrex notifications. This shouldn't cause problems irl (if we stop receiving them,
        // only part that's affected is clean up, and we're not getting new data). This means that,
        // during tests we need to trigger cleanup, by explicitly sending a shrex
        // notification for a new height.
        tracker.add_peer_for_hash(peer, new_head.header.data_hash.unwrap(), new_head.height());
        assert_peer_update(
            &poll_fn(|ctx| tracker.poll(ctx)).await.unwrap(),
            [&peer],
            [],
        );

        // we no longer track this pool, shouldn't trigger event
        let slow_notification_peer = PeerId::random();
        assert!(
            tracker
                .add_peer_for_hash(slow_notification_peer, stale_hash, stale_height)
                .is_none()
        );

        assert!(matches!(
            tracker.get_pool(stale_height),
            Err(GetPoolError::HeightTooOld)
        ));
    }

    #[async_test]
    async fn peer_selection() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let headers = g.next_many(2);

        let peer0 = PeerId::random();
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let hash0 = headers[0].header.data_hash.unwrap();
        let height0 = headers[0].height();
        let hash1 = headers[1].header.data_hash.unwrap();
        let height1 = headers[1].height();
        let invalid_hash = Hash::Sha256([3u8; 32]);

        tracker.add_peer_for_hash(peer0, hash0, height0);
        tracker.add_peer_for_hash(peer1, hash0, height0);
        store.insert(&headers[0]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        tracker.add_peer_for_hash(peer0, hash1, height1);
        tracker.add_peer_for_hash(peer1, invalid_hash, height1);
        tracker.add_peer_for_hash(peer2, hash1, height1);
        store.insert(&headers[1]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        // peer0 and peer2 sent notification for height1, they should go first
        let height_peers: HashSet<_> = tracker.get_pool(height1).unwrap().collect();
        assert_eq!(height_peers, vec_to_set(vec![&peer0, &peer2]));
    }

    #[async_test]
    async fn remove_peer() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let headers = g.next_many(2);
        let peer0 = PeerId::random();
        let peer1 = PeerId::random();
        let hash0 = headers[0].header.data_hash.unwrap();
        let height0 = headers[0].height();
        let hash1 = headers[1].header.data_hash.unwrap();
        let height1 = headers[1].height();

        tracker.add_peer_for_hash(peer0, hash0, height0);
        tracker.add_peer_for_hash(peer1, hash0, height0);

        store.insert(&headers[0]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let discovered_peers: HashSet<_> = tracker.get_pool(height0).unwrap().collect();
        assert_eq!(discovered_peers, vec_to_set(vec![&peer0, &peer1]));

        tracker.add_peer_for_hash(peer0, hash0, height1);
        tracker.add_peer_for_hash(peer1, hash1, height1);

        tracker.remove_peer(&peer0);

        let discovered_peers: Vec<_> = tracker.get_pool(height0).unwrap().collect();
        assert_eq!(discovered_peers, vec![&peer1]);

        store.insert(&headers[1]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let height_peers: Vec<_> = tracker.get_pool(height1).unwrap().collect();
        assert_eq!(height_peers, vec![&peer1]);
    }

    async fn setup_tracker(
        height: u64,
    ) -> (
        PoolTracker<InMemoryStore>,
        Arc<InMemoryStore>,
        ExtendedHeaderGenerator,
    ) {
        let (store, g) = gen_filled_store(height).await;
        let store = Arc::new(store);
        let mut tracker = PoolTracker::new(store.clone());

        // Since pool tracker starts with empty subjective_head and a pending task
        // to initialise it from the store, we poll until that finishes.
        // During normal operations this would be handled naturally as libp2p polls
        // components, but in tests we want to guarantee that the tracker is ready.
        poll_fn(|ctx| tracker.poll(ctx)).await;

        (tracker, store, g)
    }

    #[track_caller]
    fn assert_peer_update<'a>(
        ev: &'a Event,
        added: impl IntoIterator<Item = &'a PeerId>,
        blacklisted: impl IntoIterator<Item = &'a PeerId>,
    ) {
        let Event::PoolUpdate {
            blacklist_peers,
            add_peers,
        } = ev
        else {
            panic!("Invalid event type, expected PoolUpdate");
        };
        assert_eq!(
            blacklist_peers.iter().collect::<HashSet<_>>(),
            blacklisted.into_iter().collect()
        );
        assert_eq!(
            add_peers.iter().collect::<HashSet<_>>(),
            added.into_iter().collect()
        );
    }
}
