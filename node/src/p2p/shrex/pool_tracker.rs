//! Pool tracking for ShrEx protocol
//!
//! This module implements pool tracking with hash-specific pools and validation-based promotion.
//! When peers announce data availability via ShrEx/Sub, they are added to hash-specific pools.
//! Once a header is received and validated, the pool is promoted and peers are added to the
//! general discovered peers pool.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use celestia_proto::share::p2p::shrex::sub::RecentEdsNotification;
use celestia_types::{ExtendedHeader, hash::Hash};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use libp2p::PeerId;
use lumina_utils::time::{Elapsed, timeout};
use prost::Message;
use tracing::{debug, error, warn};

use crate::p2p::shrex::Event;
use crate::store::{Store, StoreError};

const ROOT_HASH_WINDOW: u64 = 10;
const POOL_VALIDATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Pool tracker for managing hash-specific and discovered peer pools
pub struct PoolTracker<S> {
    /// Height-specific pools: height -> peer pool
    hash_pools: HashMap<u64, PeerPool>,
    /// General pool of validated/discovered peers
    discovered_peers: HashSet<PeerId>,
    /// Highest known validated store height
    subjective_head: Option<u64>,
    /// Header store
    store: Arc<S>,

    new_headers_tasks:
        FuturesUnordered<BoxFuture<'static, Result<ExtendedHeader, HeaderTaskError>>>,
}

enum PeerPool {
    /// Candidates: height hasn't been validated yet, stores potential matches by hash
    Candidates((HashSet<PeerId>, HashMap<Hash, Vec<PeerId>>)),
    /// Validated: hash for this height has been accepted, all peers moved to discovered
    Validated(Hash, Vec<PeerId>),
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
            // Height is used only for clearing up pools so it's ok to pass dummy value
            // as we don't have any
            .map_err(|source| HeaderTaskError::StoreError { height: 0, source })?;
            Ok(header)
        }
        .boxed();

        Self {
            hash_pools: HashMap::new(),
            discovered_peers: HashSet::new(),
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
            .is_none_or(|stale_height| height < stale_height)
        {
            return None;
        }

        let pool = match self.hash_pools.get_mut(&height) {
            Some(pool) => pool,
            None => {
                tracing::warn!("New pool for height {height}");
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
            }
            PeerPool::Validated(validated_hash, peers) => {
                if *validated_hash == data_hash {
                    // Hash matches - add to both validated peers list and discovered peers
                    peers.push(peer_id);
                    if !self.discovered_peers.insert(peer_id) {
                        return Some(Event::PoolUpdate {
                            blacklist_peers: vec![],
                            add_peers: vec![peer_id],
                        });
                    }
                } else {
                    return Some(Event::PoolUpdate {
                        blacklist_peers: vec![peer_id],
                        add_peers: vec![],
                    });
                }
            }
        }
        None
    }

    /// Return peers suitable to be queried about hash
    ///
    /// Returns a list of peers that can be queried about the provided hash. It prioritizes peers
    /// that notified us about this specific hash, falling back to generic discovered peer pool.
    // TODO: what API?
    pub fn get_peers_for_hash(&self, data_hash: &Hash, height: u64, count: usize) -> Vec<PeerId> {
        let mut peers = Vec::with_capacity(count);

        if let Some(pool) = self.hash_pools.get(&height) {
            match pool {
                // Spec says we should use _validated_ pools for peer selection
                PeerPool::Candidates(_) => (),
                PeerPool::Validated(validated_hash, validated_peers) => {
                    if validated_hash == data_hash {
                        peers.extend(validated_peers.iter().take(count).copied());
                    }
                }
            }
        }

        // fill out the remaining slots with other discovered peers
        let mut remaining_needed = count.saturating_sub(peers.len());
        let mut discovered_peers_iter = self.discovered_peers.iter();
        while remaining_needed > 0
            && let Some(peer) = discovered_peers_iter.next()
        {
            if !peers.contains(peer) {
                peers.push(*peer);
                remaining_needed -= 1;
            }
        }

        peers
    }

    /// Get all peers that were discovered via a valid EDS notification
    pub fn discovered_peers(&self) -> Vec<PeerId> {
        self.discovered_peers.iter().copied().collect()
    }

    /// Remove peer from all pools
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.discovered_peers.remove(peer_id);

        for pool in self.hash_pools.values_mut() {
            match pool {
                PeerPool::Candidates((voted, candidates)) => {
                    voted.remove(peer_id);
                    for peers in candidates.values_mut() {
                        peers.retain(|p| p != peer_id);
                    }
                }
                PeerPool::Validated(_, peers) => {
                    peers.retain(|p| p != peer_id);
                }
            }
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

    // TODO: Errror handling
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

            let height = header.height().into();
            let Some(data_hash) = header.header.data_hash else {
                continue;
            };

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
                    let new_peers = validated_peers
                        .iter()
                        .copied()
                        .filter(|p| self.discovered_peers.insert(*p))
                        .collect();

                    let wrong_peers: Vec<PeerId> = candidates
                        .values()
                        .flat_map(|pool| pool.into_iter().cloned())
                        .collect();

                    tracing::warn!(
                        "Promoted valid pool pool for {height} with {} peers",
                        validated_peers.len()
                    );
                    *pool = PeerPool::Validated(data_hash, validated_peers);
                    return Some(Event::PoolUpdate {
                        add_peers: new_peers,
                        blacklist_peers: wrong_peers,
                    });
                }
                PeerPool::Validated(_, _) => {
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
            self.hash_pools.remove(&h);
        }
    }
}

fn stale_height_threshold(subjective_head: u64) -> u64 {
    subjective_head.saturating_sub(ROOT_HASH_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::InMemoryStore, test_utils::gen_filled_store};
    use celestia_types::test_utils::ExtendedHeaderGenerator;
    use futures::future::poll_fn;
    use lumina_utils::test_utils::async_test;
    use std::fmt::Debug;

    fn vec_to_set<T: std::hash::Hash + Eq>(v: Vec<T>) -> HashSet<T> {
        v.into_iter().collect()
    }

    async fn poll_pending_once<F, T>(mut f: F)
    where
        F: FnMut(&mut Context<'_>) -> Poll<T>,
        T: Debug,
    {
        poll_fn(|ctx| {
            assert!(f(ctx).is_pending());
            Poll::Ready(())
        })
        .await;
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

    #[async_test]
    async fn add_peer_for_hash() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let peer0 = PeerId::random();
        let hash0 = Hash::Sha256([1u8; 32]);

        let header = g.next();
        tracker.add_peer_for_hash(peer0, header.header.data_hash.unwrap(), 11);

        store.insert(header).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let hash_peers = tracker.get_peers_for_hash(&hash0, 11, 1);
        assert_eq!(hash_peers, vec![peer0]);

        let discovered_peers = tracker.discovered_peers();
        assert_eq!(discovered_peers, vec![peer0]);
    }

    #[async_test]
    async fn hash_selection() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();

        let peer0 = PeerId::random();
        let peer1_0 = PeerId::random();
        let peer1_1 = PeerId::random();
        let hash0 = Hash::Sha256([1u8; 32]);
        let hash1 = header.header.data_hash.unwrap();

        tracker.add_peer_for_hash(peer0, hash0, 11);
        tracker.add_peer_for_hash(peer1_0, hash1, 11);
        tracker.add_peer_for_hash(peer1_1, hash1, 11);

        store.insert(header).await.unwrap();

        poll_fn(|ctx| tracker.poll(ctx)).await;

        let hash_peers = vec_to_set(tracker.get_peers_for_hash(&hash1, 11, 2));
        assert_eq!(hash_peers, vec_to_set(vec![peer1_0, peer1_1]));

        let discovered_peers = vec_to_set(tracker.discovered_peers());
        assert_eq!(discovered_peers, vec_to_set(vec![peer1_0, peer1_1]));
    }

    #[async_test]
    async fn add_to_validated_pool() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();

        let peer0 = PeerId::random();
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let peer3 = PeerId::random();
        let valid_hash = header.header.data_hash.unwrap();
        let invalid_hash = Hash::Sha256([2u8; 32]);

        tracker.add_peer_for_hash(peer0, valid_hash, 11);

        store.insert(header).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let discovered_peers = tracker.discovered_peers();
        assert_eq!(discovered_peers, vec![peer0]);

        tracker.add_peer_for_hash(peer1, valid_hash, 11);
        tracker.add_peer_for_hash(peer2, valid_hash, 11);
        tracker.add_peer_for_hash(peer3, invalid_hash, 11);

        let discovered_peers = vec_to_set(tracker.discovered_peers());
        assert_eq!(discovered_peers, vec_to_set(vec![peer0, peer1, peer2]));
    }

    #[async_test]
    async fn duplicate_votes() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let header = g.next();

        let valid_hash = header.header.data_hash.unwrap();
        let peer0 = PeerId::random();
        let invalid_hash0 = Hash::Sha256([2u8; 32]);
        let peer1 = PeerId::random();
        let invalid_hash1 = Hash::Sha256([3u8; 32]);

        // only first vote should be counted
        tracker.add_peer_for_hash(peer0, valid_hash, 11);
        tracker.add_peer_for_hash(peer0, invalid_hash0, 11);

        tracker.add_peer_for_hash(peer1, invalid_hash1, 11);
        tracker.add_peer_for_hash(peer1, valid_hash, 11);

        store.insert(header).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let discovered_peers = tracker.discovered_peers();
        assert_eq!(discovered_peers, vec![peer0]);
    }

    #[async_test]
    async fn ignore_old_heights() {
        let (mut tracker, store, mut g) = setup_tracker(1).await;
        let old_header = g.next();
        let headers = g.next_many(10);

        let valid_stale_hash = old_header.header.data_hash.unwrap();
        let old_peer = PeerId::random();

        store.insert(old_header).await.unwrap();
        store.insert(headers).await.unwrap();

        // TODO: currently we're polled by shrex/sub notifications
        // how to also keep up with the store?
        tracker.add_peer_for_hash(old_peer, valid_stale_hash, 2);

        assert!(tracker.discovered_peers().is_empty());
    }

    #[async_test]
    async fn eviction() {
        let (mut tracker, store, mut g) = setup_tracker(3).await;
        let old_header = g.next();
        let headers = g.next_many(10);

        let stale_hash = old_header.header.data_hash.unwrap();
        let old_peer = PeerId::random();

        tracker.add_peer_for_hash(old_peer, stale_hash, 4);
        tracker.validate_pool(stale_hash, 4);
        store.insert(old_header).await.unwrap();
        store.insert(headers).await.unwrap();
        //for h in 5..=5 + ROOT_HASH_WINDOW { tracker.validate_pool(Hash::Sha256([h as u8; 32]), h); }
        let discovered_peers = tracker.discovered_peers();
        assert_eq!(discovered_peers, vec![old_peer]);

        // we no longer track this pool, should be ignored
        let slow_notification_peer = PeerId::random();
        tracker.add_peer_for_hash(slow_notification_peer, stale_hash, 4);

        let discovered_peers = tracker.discovered_peers();
        assert_eq!(discovered_peers, vec![old_peer]);
    }

    #[async_test]
    async fn peer_selection() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let headers = g.next_many(2);

        let peer0 = PeerId::random();
        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        let hash0 = headers[0].header.data_hash.unwrap();
        let hash1 = headers[1].header.data_hash.unwrap();
        let invalid_hash = Hash::Sha256([3u8; 32]);

        tracker.add_peer_for_hash(peer0, hash0, 11);
        tracker.add_peer_for_hash(peer1, hash0, 11);
        store.insert(&headers[0]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        tracker.add_peer_for_hash(peer0, hash1, 12);
        tracker.add_peer_for_hash(peer1, invalid_hash, 12);
        tracker.add_peer_for_hash(peer2, hash1, 12);
        store.insert(&headers[1]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        println!("P0 {peer0}");
        println!("P1 {peer1}");
        println!("P2 {peer2}");
        // peer0 and peer2 sent notification for hash1, they should go first
        let hash_peers = tracker.get_peers_for_hash(&hash1, 12, 3);
        println!("HP {hash_peers:#?}");
        assert_eq!(hash_peers[2], peer1);
        let preferred_peers = HashSet::from_iter(hash_peers[0..=1].iter().cloned());
        assert_eq!(preferred_peers, vec_to_set(vec![peer0, peer2]));
    }

    #[async_test]
    async fn remove_peer() {
        let (mut tracker, store, mut g) = setup_tracker(10).await;
        let headers = g.next_many(2);
        let peer0 = PeerId::random();
        let peer1 = PeerId::random();
        let hash0 = headers[0].header.data_hash.unwrap();
        let hash1 = headers[1].header.data_hash.unwrap();

        tracker.add_peer_for_hash(peer0, hash0, 11);
        tracker.add_peer_for_hash(peer1, hash0, 11);

        store.insert(&headers[0]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let discovered_peers = vec_to_set(tracker.discovered_peers());
        assert_eq!(discovered_peers, vec_to_set(vec![peer0, peer1]));

        tracker.add_peer_for_hash(peer0, hash0, 12);
        tracker.add_peer_for_hash(peer1, hash1, 12);

        tracker.remove_peer(&peer0);

        let discovered_peers = vec_to_set(tracker.discovered_peers());
        assert_eq!(discovered_peers, vec_to_set(vec![peer1]));

        store.insert(&headers[1]).await.unwrap();
        poll_fn(|ctx| tracker.poll(ctx)).await;

        let discovered_peers = vec_to_set(tracker.discovered_peers());
        assert_eq!(discovered_peers, vec_to_set(vec![peer1]));

        let hash_peers = tracker.get_peers_for_hash(&hash1, 12, 1);
        assert_eq!(hash_peers, vec![peer1]);
    }
}
