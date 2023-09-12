use libp2p::PeerId;
use std::collections::HashSet;
use std::sync::RwLock;

pub struct PeerTracker {
    inner: RwLock<Inner>,
}

struct Inner {
    peers: HashSet<PeerId>,
}

impl PeerTracker {
    pub fn new() -> Self {
        PeerTracker {
            inner: RwLock::new(Inner {
                peers: HashSet::new(),
            }),
        }
    }

    pub fn add(&self, peer: PeerId) {
        let mut inner = self.inner.write().unwrap();
        inner.peers.insert(peer);
    }

    pub fn remove(&self, peer: PeerId) {
        let mut inner = self.inner.write().unwrap();
        inner.peers.remove(&peer);
    }

    pub fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_best(&self) -> Option<PeerId> {
        // TODO: Implement peer score and return the best.
        let inner = self.inner.read().unwrap();
        inner.peers.iter().next().copied()
    }
}

impl Default for PeerTracker {
    fn default() -> Self {
        PeerTracker::new()
    }
}
