//! Primitives related to tracking the state of peers in the network.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use libp2p::ping;
use libp2p::{swarm::ConnectionId, PeerId};
use lumina_utils::time::Instant;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use tokio::sync::watch;
use tracing::info;

use crate::events::{EventPublisher, NodeEvent};

/// How ofter garbage collector should be called.
pub(crate) const GC_INTERVAL: Duration = Duration::from_secs(30);
/// How much time a `Peer` needs to be disconnected to get expired.
const EXPIRED_AFTER: Duration = Duration::from_secs(120);

/// Keeps track various information about peers.
#[derive(Debug)]
pub(crate) struct PeerTracker {
    peers: HashMap<PeerId, Peer>,
    protect_counter: HashMap<u32, usize>,
    info_tx: watch::Sender<PeerTrackerInfo>,
    event_pub: EventPublisher,
}

/// Statistics of the connected peers
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PeerTrackerInfo {
    /// Number of the connected peers.
    pub num_connected_peers: u64,
    /// Number of the connected trusted peers.
    pub num_connected_trusted_peers: u64,
    /// Number of the connected full nodes.
    // This is used by `SwarmManager` in order to trigger `peers_health_check`.
    pub num_connected_full_nodes: u64,
    /// Number of the connected archival nodes.
    // This is used by `SwarmManager` in order to trigger `peers_health_check`.
    pub num_connected_archival_nodes: u64,
}

#[derive(Debug)]
pub(crate) struct Peer {
    id: PeerId,
    connections: HashMap<ConnectionId, ConnectionInfo>,
    protected: HashSet<u32>,
    trusted: bool,
    archival: bool,
    node_kind: NodeKind,
    disconnected_at: Option<Instant>,
}

#[derive(Debug, Default)]
struct ConnectionInfo {
    ping: Option<Duration>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum NodeKind {
    #[default]
    Unknown,
    Bridge,
    Full,
    Light,
}

impl NodeKind {
    fn from_agent_version(s: &str) -> NodeKind {
        let mut s = s.split('/');

        match s.next() {
            Some("lumina") => NodeKind::Light,
            Some("celestia-node") => match s.nth(1) {
                Some("bridge") => NodeKind::Bridge,
                Some("full") => NodeKind::Full,
                Some("light") => NodeKind::Light,
                _ => NodeKind::Unknown,
            },
            _ => NodeKind::Unknown,
        }
    }

    pub(crate) fn is_full(&self) -> bool {
        matches!(self, NodeKind::Full | NodeKind::Bridge)
    }
}

impl Peer {
    fn new(id: PeerId) -> Self {
        Peer {
            id,
            connections: HashMap::new(),
            protected: HashSet::new(),
            trusted: false,
            archival: false,
            node_kind: NodeKind::Unknown,
            // We start as disconnected
            disconnected_at: Some(Instant::now()),
        }
    }

    pub(crate) fn id(&self) -> &PeerId {
        &self.id
    }

    pub(crate) fn is_connected(&self) -> bool {
        !self.connections.is_empty()
    }

    pub(crate) fn is_trusted(&self) -> bool {
        self.trusted
    }

    pub(crate) fn is_protected(&self) -> bool {
        !self.protected.is_empty()
    }

    pub(crate) fn is_protected_for(&self, tag: u32) -> bool {
        self.protected.contains(&tag)
    }

    pub(crate) fn is_archival(&self) -> bool {
        self.archival
    }

    pub(crate) fn is_full(&self) -> bool {
        self.node_kind.is_full()
    }

    #[allow(dead_code)]
    pub(crate) fn node_kind(&self) -> NodeKind {
        self.node_kind
    }

    pub(crate) fn best_ping(&self) -> Option<Duration> {
        self.connections
            .iter()
            .flat_map(|(_, conn_info)| conn_info.ping)
            .min()
    }
}

impl PeerTracker {
    /// Constructs an empty PeerTracker.
    pub(crate) fn new(event_pub: EventPublisher) -> Self {
        PeerTracker {
            peers: HashMap::new(),
            protect_counter: HashMap::new(),
            info_tx: watch::channel(PeerTrackerInfo::default()).0,
            event_pub,
        }
    }

    /// Returns the current [`PeerTrackerInfo`].
    pub(crate) fn info(&self) -> PeerTrackerInfo {
        self.info_tx.borrow().to_owned()
    }

    /// Returns a watcher for any [`PeerTrackerInfo`] changes.
    pub(crate) fn info_watcher(&self) -> watch::Receiver<PeerTrackerInfo> {
        self.info_tx.subscribe()
    }

    pub(crate) fn peer(&self, peer_id: &PeerId) -> Option<&Peer> {
        self.peers.get(peer_id)
    }

    pub(crate) fn peers(&self) -> impl Iterator<Item = &Peer> {
        self.peers.values()
    }

    pub(crate) fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.peer(peer_id).is_some_and(|p| p.is_connected())
    }

    pub(crate) fn is_protected(&self, peer_id: &PeerId) -> bool {
        self.peer(peer_id).is_some_and(|p| p.is_protected())
    }

    pub(crate) fn is_protected_for(&self, peer_id: &PeerId, tag: u32) -> bool {
        self.peer(peer_id).is_some_and(|p| p.is_protected_for(tag))
    }

    /// Adds a peer ID.
    ///
    /// Returns `true` if peer was not known from before.
    pub(crate) fn add_peer_id(&mut self, peer_id: &PeerId) -> bool {
        match self.peers.entry(*peer_id) {
            Entry::Vacant(entry) => {
                entry.insert(Peer::new(*peer_id));
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    /// Sets peer as trusted.
    pub(crate) fn set_trusted(&mut self, peer_id: &PeerId, is_trusted: bool) {
        let peer = self
            .peers
            .entry(*peer_id)
            .or_insert_with(|| Peer::new(*peer_id));

        if peer.trusted == is_trusted {
            // Nothing to be done
            return;
        }

        peer.trusted = is_trusted;

        // If peer was already connected, then `num_connected_trusted_peers`
        // needs to be adjusted based on the new information.
        if peer.is_connected() {
            self.info_tx.send_modify(|tracker_info| {
                if is_trusted {
                    tracker_info.num_connected_trusted_peers += 1;
                } else {
                    tracker_info.num_connected_trusted_peers -= 1;
                }
            });
        }
    }

    /// Add protect flag to the peer.
    ///
    /// Tag allows different reasons of protection without interfering with one another.
    ///
    /// Returns `true` if peer changes from unprotected state to protected.
    pub(crate) fn protect(&mut self, peer_id: &PeerId, tag: u32) -> bool {
        let peer = self
            .peers
            .entry(*peer_id)
            .or_insert_with(|| Peer::new(*peer_id));
        let was_protected = peer.is_protected();

        if peer.protected.insert(tag) {
            *self.protect_counter.entry(tag).or_default() += 1;
            info!("Protect peer {peer_id} with {tag} tag");
        }

        !was_protected
    }

    /// Remove protect flag from the peer.
    ///
    /// Tag allows different reasons of protection without interfering with one another.
    ///
    /// Returns `true` if peer changes from protected state to unprotected.
    pub(crate) fn unprotect(&mut self, peer_id: &PeerId, tag: u32) -> bool {
        let Some(peer) = self.peers.get_mut(peer_id) else {
            return false;
        };

        let was_protected = peer.is_protected();

        if peer.protected.remove(&tag) {
            *self
                .protect_counter
                .get_mut(&tag)
                .expect("protected flag was set but not counted") -= 1;

            info!("Unprotect peer {peer_id} with {tag} tag");
        }

        // Return true is `protected` state changed
        was_protected && !peer.is_protected()
    }

    pub(crate) fn protected_len(&self, tag: u32) -> usize {
        self.protect_counter.get(&tag).copied().unwrap_or(0)
    }

    /// Add an active connection of a peer.
    pub(crate) fn add_connection(&mut self, peer_id: &PeerId, connection_id: ConnectionId) {
        let peer = self
            .peers
            .entry(*peer_id)
            .or_insert_with(|| Peer::new(*peer_id));
        let prev_connected = peer.is_connected();

        peer.connections
            .insert(connection_id, ConnectionInfo::default());

        // If peer was not already connected from before
        if !prev_connected {
            increment_connected_peers(&self.info_tx, peer);
            peer.disconnected_at.take();

            self.event_pub.send(NodeEvent::PeerConnected {
                id: *peer_id,
                trusted: peer.trusted,
            });
        }
    }

    /// Remove a connection from a peer.
    pub(crate) fn remove_connection(&mut self, peer_id: &PeerId, connection_id: ConnectionId) {
        let Some(peer) = self.peers.get_mut(peer_id) else {
            return;
        };

        peer.connections.retain(|id, _| *id != connection_id);

        // If this is the last connection from the peer.
        if !peer.is_connected() {
            decrement_connected_peers(&self.info_tx, peer);
            peer.node_kind = NodeKind::Unknown;
            peer.archival = false;
            peer.disconnected_at = Some(Instant::now());

            self.event_pub.send(NodeEvent::PeerDisconnected {
                id: peer_id.to_owned(),
                trusted: peer.trusted,
            });
        }
    }

    pub(crate) fn on_agent_version(&mut self, peer_id: &PeerId, agent_version: &str) {
        let peer = self
            .peers
            .entry(*peer_id)
            .or_insert_with(|| Peer::new(*peer_id));
        let new_node_kind = NodeKind::from_agent_version(agent_version);

        let was_full = peer.node_kind.is_full();
        let is_full = new_node_kind.is_full();
        peer.node_kind = new_node_kind;

        self.info_tx
            .send_if_modified(|tracker_info| match (was_full, is_full) {
                (true, false) => {
                    tracker_info.num_connected_full_nodes -= 1;
                    true
                }
                (false, true) => {
                    tracker_info.num_connected_full_nodes += 1;
                    true
                }
                _ => false,
            });
    }

    pub(crate) fn on_ping_event(&mut self, ev: &ping::Event) {
        if let Some(peer) = self.peers.get_mut(&ev.peer) {
            if let Some(conn_info) = peer.connections.get_mut(&ev.connection) {
                conn_info.ping = ev.result.as_ref().ok().copied();
            }
        }
    }

    pub(crate) fn mark_as_archival(&mut self, peer_id: &PeerId) {
        let peer = self
            .peers
            .entry(*peer_id)
            .or_insert_with(|| Peer::new(*peer_id));

        if !peer.archival {
            peer.archival = true;

            self.info_tx.send_modify(|tracker_info| {
                tracker_info.num_connected_archival_nodes += 1;
            });
        }
    }

    pub(crate) fn connections(&self, peer_id: &PeerId) -> impl Iterator<Item = ConnectionId> + '_ {
        self.peer(peer_id)
            .map(|peer| peer.connections.keys().copied())
            .into_iter()
            .flatten()
    }

    /// Returns all connections.
    pub(crate) fn all_connections(&self) -> impl Iterator<Item = (&PeerId, ConnectionId)> + '_ {
        self.peers()
            .filter(|peer| peer.is_connected())
            .flat_map(|peer| {
                peer.connections
                    .keys()
                    .copied()
                    .map(|conn| (peer.id(), conn))
            })
    }

    /// Returns one of the best peers.
    pub(crate) fn best_peer(&self) -> Option<PeerId> {
        const MAX_PEER_SAMPLE: usize = 128;

        // TODO: Implement peer score and return the best.
        let mut peers = self
            .peers
            .iter()
            .filter(|(_, peer)| peer.is_connected())
            .take(MAX_PEER_SAMPLE)
            .map(|(peer_id, _)| peer_id)
            .collect::<SmallVec<[_; MAX_PEER_SAMPLE]>>();

        peers.shuffle(&mut rand::thread_rng());

        peers.first().copied().copied()
    }

    pub(crate) fn gc(&mut self) {
        self.peers.retain(|_, peer| {
            // We keep:
            //
            // * Connected peers
            // * Protected peers
            // * Recently disconnected peers
            peer.is_connected()
                || peer.is_protected()
                || peer
                    .disconnected_at
                    .is_none_or(|tm| tm.elapsed() <= EXPIRED_AFTER)
        });
    }
}

fn increment_connected_peers(info_tx: &watch::Sender<PeerTrackerInfo>, peer: &Peer) {
    info_tx.send_modify(|tracker_info| {
        tracker_info.num_connected_peers += 1;

        if peer.trusted {
            tracker_info.num_connected_trusted_peers += 1;
        }
    });
}

fn decrement_connected_peers(info_tx: &watch::Sender<PeerTrackerInfo>, peer: &Peer) {
    info_tx.send_modify(|tracker_info| {
        tracker_info.num_connected_peers -= 1;

        if peer.trusted {
            tracker_info.num_connected_trusted_peers -= 1;
        }

        if peer.archival {
            tracker_info.num_connected_archival_nodes -= 1;
        }

        if peer.node_kind.is_full() {
            tracker_info.num_connected_full_nodes -= 1;
        }
    });
}

#[cfg(test)]
mod tests {
    use crate::events::EventChannel;

    use super::*;

    #[test]
    fn trust_before_connect() {
        let event_channel = EventChannel::new();
        let mut tracker = PeerTracker::new(event_channel.publisher());
        let mut watcher = tracker.info_watcher();
        let peer_id = PeerId::random();

        assert!(!watcher.has_changed().unwrap());

        tracker.set_trusted(&peer_id, true);
        assert!(!watcher.has_changed().unwrap());

        tracker.add_connection(&peer_id, ConnectionId::new_unchecked(1));
        assert!(tracker.is_connected(&peer_id));
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(info.num_connected_peers, 1);
        assert_eq!(info.num_connected_trusted_peers, 1);
    }

    #[test]
    fn trust_after_connect() {
        let event_channel = EventChannel::new();
        let mut tracker = PeerTracker::new(event_channel.publisher());
        let mut watcher = tracker.info_watcher();
        let peer_id = PeerId::random();

        assert!(!watcher.has_changed().unwrap());

        tracker.add_connection(&peer_id, ConnectionId::new_unchecked(1));
        assert!(tracker.is_connected(&peer_id));
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(info.num_connected_peers, 1);
        assert_eq!(info.num_connected_trusted_peers, 0);

        tracker.set_trusted(&peer_id, true);
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(info.num_connected_peers, 1);
        assert_eq!(info.num_connected_trusted_peers, 1);
    }

    #[test]
    fn untrust_after_connect() {
        let event_channel = EventChannel::new();
        let mut tracker = PeerTracker::new(event_channel.publisher());
        let mut watcher = tracker.info_watcher();
        let peer_id = PeerId::random();

        assert!(!watcher.has_changed().unwrap());

        tracker.set_trusted(&peer_id, true);
        assert!(!watcher.has_changed().unwrap());

        tracker.add_connection(&peer_id, ConnectionId::new_unchecked(1));
        assert!(tracker.is_connected(&peer_id));
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(info.num_connected_peers, 1);
        assert_eq!(info.num_connected_trusted_peers, 1);

        tracker.set_trusted(&peer_id, false);
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(info.num_connected_peers, 1);
        assert_eq!(info.num_connected_trusted_peers, 0);
    }

    #[test]
    fn tracker_info() {
        let event_channel = EventChannel::new();
        let mut tracker = PeerTracker::new(event_channel.publisher());
        let mut watcher = tracker.info_watcher();
        let peer_id = PeerId::random();

        tracker.add_connection(&peer_id, ConnectionId::new_unchecked(1));
        assert!(tracker.is_connected(&peer_id));
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(
            info,
            PeerTrackerInfo {
                num_connected_peers: 1,
                num_connected_trusted_peers: 0,
                num_connected_full_nodes: 0,
                num_connected_archival_nodes: 0,
            }
        );

        tracker.mark_as_archival(&peer_id);
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(
            info,
            PeerTrackerInfo {
                num_connected_peers: 1,
                num_connected_trusted_peers: 0,
                num_connected_full_nodes: 0,
                num_connected_archival_nodes: 1,
            }
        );

        tracker.mark_as_archival(&peer_id);
        assert!(!watcher.has_changed().unwrap());

        tracker.on_agent_version(&peer_id, "celestia-node/celestia/full/v0.24.1/fb95d45");
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(
            info,
            PeerTrackerInfo {
                num_connected_peers: 1,
                num_connected_trusted_peers: 0,
                num_connected_full_nodes: 1,
                num_connected_archival_nodes: 1,
            }
        );

        tracker.on_agent_version(&peer_id, "celestia-node/celestia/full/v0.24.1/fb95d45");
        assert!(!watcher.has_changed().unwrap());

        // Peer gets disconnected
        tracker.remove_connection(&peer_id, ConnectionId::new_unchecked(1));
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(
            info,
            PeerTrackerInfo {
                num_connected_peers: 0,
                num_connected_trusted_peers: 0,
                num_connected_full_nodes: 0,
                num_connected_archival_nodes: 0,
            }
        );

        // Peer gets reconnected
        tracker.add_connection(&peer_id, ConnectionId::new_unchecked(2));
        assert!(tracker.is_connected(&peer_id));
        assert!(watcher.has_changed().unwrap());
        let info = watcher.borrow_and_update().to_owned();
        assert_eq!(
            info,
            PeerTrackerInfo {
                num_connected_peers: 1,
                num_connected_trusted_peers: 0,
                num_connected_full_nodes: 0,
                num_connected_archival_nodes: 0,
            }
        );
    }

    #[test]
    fn protect() {
        let peer_id = PeerId::random();
        let event_channel = EventChannel::new();
        let mut tracker = PeerTracker::new(event_channel.publisher());

        // Unknown peers are always unprotected, so state doesn't change
        assert!(!tracker.is_protected(&peer_id));
        assert!(!tracker.unprotect(&peer_id, 0));
        assert_eq!(tracker.protected_len(0), 0);

        // Now state changed from unprotected to protected
        assert!(!tracker.is_protected_for(&peer_id, 0));
        assert!(tracker.protect(&peer_id, 0));
        assert!(tracker.is_protected(&peer_id));
        assert!(tracker.is_protected_for(&peer_id, 0));
        assert_eq!(tracker.protected_len(0), 1);
        // Adding more tags doesn't change the state
        assert!(!tracker.is_protected_for(&peer_id, 1));
        assert!(!tracker.protect(&peer_id, 1));
        assert!(tracker.is_protected(&peer_id));
        assert!(tracker.is_protected_for(&peer_id, 1));
        assert_eq!(tracker.protected_len(1), 1);

        // Adding an existing tag to a peer doesn't change the counter
        assert!(!tracker.protect(&peer_id, 0));
        assert_eq!(tracker.protected_len(0), 1);
        // Adding a tag to a peer must increase the counter
        assert!(tracker.protect(&PeerId::random(), 0));
        assert_eq!(tracker.protected_len(0), 2);

        // Removing only some of the tags doesn't change the state
        assert!(!tracker.unprotect(&peer_id, 0));
        assert!(!tracker.is_protected_for(&peer_id, 0));
        assert!(tracker.is_protected(&peer_id));
        assert_eq!(tracker.protected_len(0), 1);
        // Removing all tags, changes the state from protected to unprotected
        assert!(tracker.unprotect(&peer_id, 1));
        assert!(!tracker.is_protected_for(&peer_id, 1));
        assert!(!tracker.is_protected(&peer_id));
        assert_eq!(tracker.protected_len(1), 0);
    }

    #[test]
    fn node_kind() {
        assert_eq!(
            NodeKind::from_agent_version("lumina/celestia/0.14.0"),
            NodeKind::Light
        );

        assert_eq!(
            NodeKind::from_agent_version("celestia-node/celestia/bridge/v0.24.1/fb95d45"),
            NodeKind::Bridge
        );

        assert_eq!(
            NodeKind::from_agent_version("celestia-node/celestia/full/v0.24.1/fb95d45"),
            NodeKind::Full
        );

        assert_eq!(
            NodeKind::from_agent_version("celestia-node/celestia/light/v0.24.1/fb95d45"),
            NodeKind::Light
        );

        assert_eq!(
            NodeKind::from_agent_version("probelab-node/celestia/ant/v0.1.0"),
            NodeKind::Unknown
        );
    }
}
