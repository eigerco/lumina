use std::borrow::Borrow;

use dashmap::DashSet;
use libp2p::PeerId;

pub struct PeerTracker {
    peers: DashSet<PeerId>,
}

impl PeerTracker {
    pub fn new() -> Self {
        PeerTracker {
            peers: DashSet::new(),
        }
    }

    pub fn add(&self, peer: PeerId) {
        self.peers.insert(peer);
    }

    pub fn add_many<I, P>(&self, peers: I)
    where
        I: IntoIterator<Item = P>,
        P: Borrow<PeerId>,
    {
        for peer in peers {
            self.add(*peer.borrow());
        }
    }

    pub fn remove(&self, peer: PeerId) {
        self.peers.remove(&peer);
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    pub fn best_peer(&self) -> Option<PeerId> {
        // TODO: Implement peer score and return the best.
        self.peers.iter().next().map(|v| v.key().to_owned())
    }

    pub fn best_n_peers(&self, limit: usize) -> Option<Vec<PeerId>> {
        // TODO: Implement peer score and return the best N peers.
        let peers = self
            .peers
            .iter()
            .take(limit)
            .map(|v| v.key().to_owned())
            // collect instead of returning an iter to not block the dashmap
            .collect::<Vec<_>>();

        if peers.is_empty() {
            None
        } else {
            Some(peers)
        }
    }
}

impl Default for PeerTracker {
    fn default() -> Self {
        PeerTracker::new()
    }
}
