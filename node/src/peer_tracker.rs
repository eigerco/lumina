use std::borrow::Borrow;

use dashmap::mapref::entry::Entry;
use dashmap::mapref::one::RefMut;
use dashmap::DashMap;
use libp2p::{identify, Multiaddr, PeerId};

#[derive(Debug)]
pub struct PeerTracker {
    peers: DashMap<PeerId, PeerInfo>,
}

#[derive(Debug)]
struct PeerInfo {
    addrs: Vec<Multiaddr>,
    state: PeerState,
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
    pub fn new() -> Self {
        PeerTracker {
            peers: DashMap::new(),
        }
    }

    pub fn maybe_discovered(&self, peer: PeerId) -> bool {
        match self.peers.entry(peer) {
            Entry::Vacant(entry) => {
                entry.insert(PeerInfo {
                    addrs: Vec::new(),
                    state: PeerState::Discovered,
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
            addrs: Vec::new(),
            state: PeerState::Discovered,
        })
    }

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

    pub fn connected(&self, peer: PeerId, address: impl Into<Option<Multiaddr>>) {
        let mut state = self.get(peer);

        if let Some(address) = address.into() {
            if !state.addrs.contains(&address) {
                state.addrs.push(address);
            }
        }

        state.state = PeerState::Connected;
    }

    pub fn disconnected(&self, peer: PeerId) {
        let mut state = self.get(peer);

        if state.addrs.is_empty() {
            state.state = PeerState::Discovered;
        } else {
            state.state = PeerState::AddressesFound;
        }
    }

    pub fn identified(&self, peer: PeerId, info: &identify::Info) {
        let mut state = self.get(peer);

        for addr in &info.listen_addrs {
            if !state.addrs.contains(addr) {
                state.addrs.push(addr.to_owned());
            }
        }

        state.state = PeerState::Identified
    }

    pub fn is_anyone_connected(&self) -> bool {
        self.peers.iter().any(|pair| pair.value().is_connected())
    }

    pub fn is_connected(&self, peer: PeerId) -> bool {
        self.get(peer).is_connected()
    }

    pub fn addresses(&self, peer: PeerId) -> Vec<Multiaddr> {
        self.get(peer).addrs.clone()
    }

    pub fn remove(&self, peer: PeerId) {
        self.peers.remove(&peer);
    }

    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|pair| pair.value().is_connected())
            .map(|pair| pair.key().to_owned())
            .collect()
    }

    pub fn best_peer(&self) -> Option<PeerId> {
        // TODO: Implement peer score and return the best.
        self.peers
            .iter()
            .find(|pair| pair.value().is_connected())
            .map(|pair| pair.key().to_owned())
    }

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
}

impl Default for PeerTracker {
    fn default() -> Self {
        PeerTracker::new()
    }
}
