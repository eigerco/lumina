use std::borrow::Borrow;

use dashmap::mapref::entry::Entry;
use dashmap::mapref::one::RefMut;
use dashmap::DashMap;
use libp2p::{identify, swarm::ConnectionId, Multiaddr, PeerId};
use smallvec::SmallVec;
use tokio::sync::watch;

/// Keeps track various information about peers.
#[derive(Debug)]
pub struct PeerTracker {
    peers: DashMap<PeerId, PeerInfo>,
    info_tx: watch::Sender<PeerTrackerInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct PeerTrackerInfo {
    /// Number of the connected peers.
    pub num_connected_peers: u64,
    /// Number of the connected trusted peers.
    pub num_connected_trusted_peers: u64,
}

#[derive(Debug)]
struct PeerInfo {
    state: PeerState,
    addrs: SmallVec<[Multiaddr; 4]>,
    connections: SmallVec<[ConnectionId; 1]>,
    trusted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerState {
    Discovered,
    AddressesFound,
    Connected,
    Identified,
}

impl PeerInfo {
    fn is_connected(&self) -> bool {
        matches!(self.state, PeerState::Connected | PeerState::Identified)
    }
}

impl PeerTracker {
    /// Constructs an empty PeerTracker.
    pub fn new() -> Self {
        PeerTracker {
            peers: DashMap::new(),
            info_tx: watch::channel(PeerTrackerInfo::default()).0,
        }
    }

    /// Returns the current [`PeerTrackerInfo`].
    pub fn info(&self) -> PeerTrackerInfo {
        self.info_tx.borrow().to_owned()
    }

    /// Returns a watcher for any [`PeerTrackerInfo`] changes.
    pub fn info_watcher(&self) -> watch::Receiver<PeerTrackerInfo> {
        self.info_tx.subscribe()
    }

    /// Sets peer as discovered if this is it's first appearance.
    ///
    /// Returns `true` if peer was not known from before.
    pub fn set_maybe_discovered(&self, peer: PeerId) -> bool {
        match self.peers.entry(peer) {
            Entry::Vacant(entry) => {
                entry.insert(PeerInfo {
                    state: PeerState::Discovered,
                    addrs: SmallVec::new(),
                    connections: SmallVec::new(),
                    trusted: false,
                });
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    /// Get the `PeerInfo` of the peer.
    ///
    /// If peer is not found it is added as `PeerState::Discovered`.
    fn get(&self, peer: PeerId) -> RefMut<PeerId, PeerInfo> {
        self.peers.entry(peer).or_insert_with(|| PeerInfo {
            state: PeerState::Discovered,
            addrs: SmallVec::new(),
            connections: SmallVec::new(),
            trusted: false,
        })
    }

    /// Add an address for a peer.
    pub fn add_addresses<I, A>(&self, peer: PeerId, addrs: I)
    where
        I: IntoIterator<Item = A>,
        A: Borrow<Multiaddr>,
    {
        let mut state = self.get(peer);

        for addr in addrs {
            let addr = addr.borrow();

            if !state.addrs.contains(addr) {
                state.addrs.push(addr.to_owned());
            }
        }

        // Upgrade state
        if state.state == PeerState::Discovered && !state.addrs.is_empty() {
            state.state = PeerState::AddressesFound;
        }
    }

    /// Sets peer as trusted.
    pub fn set_trusted(&self, peer: PeerId) {
        self.get(peer).value_mut().trusted = true;
    }

    /// Sets peer as connected.
    pub fn set_connected(
        &self,
        peer: PeerId,
        connection_id: ConnectionId,
        address: impl Into<Option<Multiaddr>>,
    ) {
        let mut peer_info = self.get(peer);

        if let Some(address) = address.into() {
            if !peer_info.addrs.contains(&address) {
                peer_info.addrs.push(address);
            }
        }

        // This is needed to avoid downgrading `Identified` state to `Connected`
        if !peer_info.is_connected() {
            peer_info.state = PeerState::Connected;
        }

        peer_info.connections.push(connection_id);

        // If this is the first connection from the peer
        if peer_info.connections.len() == 1 {
            increment_connected_peers(&self.info_tx, peer_info.trusted);
        }
    }

    /// Sets peer as disconnected if `connection_id` was the last connection.
    ///
    /// Returns `true` if was set to disconnected.
    pub fn set_maybe_disconnected(&self, peer: PeerId, connection_id: ConnectionId) -> bool {
        let mut peer_info = self.get(peer);

        peer_info.connections.retain(|id| *id != connection_id);

        // If this is the last connection from the peer
        if peer_info.connections.is_empty() {
            decrement_connected_peers(&self.info_tx, peer_info.trusted);

            if peer_info.addrs.is_empty() {
                peer_info.state = PeerState::Discovered;
            } else {
                peer_info.state = PeerState::AddressesFound;
            }

            true
        } else {
            false
        }
    }

    /// Sets peer as identified.
    pub fn set_identified(&self, peer: PeerId, info: &identify::Info) {
        let mut peer_info = self.get(peer);

        for addr in &info.listen_addrs {
            if !peer_info.addrs.contains(addr) {
                peer_info.addrs.push(addr.to_owned());
            }
        }

        peer_info.state = PeerState::Identified;
    }

    /// Returns true if peer is connected.
    pub fn is_connected(&self, peer: PeerId) -> bool {
        self.get(peer).is_connected()
    }

    /// Returns the addresses of the peer.
    pub fn addresses(&self, peer: PeerId) -> SmallVec<[Multiaddr; 4]> {
        self.get(peer).addrs.clone()
    }

    /// Removes a peer.
    pub fn remove(&self, peer: PeerId) {
        self.peers.remove(&peer);
    }

    /// Returns connected peers.
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|pair| pair.value().is_connected())
            .map(|pair| pair.key().to_owned())
            .collect()
    }

    /// Returns one of the best peers.
    pub fn best_peer(&self) -> Option<PeerId> {
        // TODO: Implement peer score and return the best.
        self.peers
            .iter()
            .find(|pair| pair.value().is_connected())
            .map(|pair| pair.key().to_owned())
    }

    /// Returns up to N amount of best peers.
    pub fn best_n_peers(&self, limit: usize) -> Vec<PeerId> {
        // TODO: Implement peer score and return the best N peers.
        self.peers
            .iter()
            .filter(|pair| pair.value().is_connected())
            .take(limit)
            .map(|pair| pair.key().to_owned())
            // collect instead of returning an iter to not block the dashmap
            .collect()
    }

    /// Returns up to N amount of trusted peers.
    pub fn trusted_n_peers(&self, limit: usize) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|pair| pair.value().is_connected() && pair.value().trusted)
            .take(limit)
            .map(|pair| pair.key().to_owned())
            // collect instead of returning an iter to not block the dashmap
            .collect()
    }
}

impl Default for PeerTracker {
    fn default() -> Self {
        PeerTracker::new()
    }
}

fn increment_connected_peers(info_tx: &watch::Sender<PeerTrackerInfo>, trusted: bool) {
    info_tx.send_modify(|tracker_info| {
        tracker_info.num_connected_peers += 1;

        if trusted {
            tracker_info.num_connected_trusted_peers += 1;
        }
    });
}

fn decrement_connected_peers(info_tx: &watch::Sender<PeerTrackerInfo>, trusted: bool) {
    info_tx.send_modify(|tracker_info| {
        tracker_info.num_connected_peers -= 1;

        if trusted {
            tracker_info.num_connected_trusted_peers -= 1;
        }
    });
}
