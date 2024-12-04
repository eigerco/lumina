use std::any::TypeId;
use std::time::Duration;

use blockstore::Blockstore;
use libp2p::identity::Keypair;
use libp2p::Multiaddr;
use tracing::{info, warn};

use crate::blockstore::InMemoryBlockstore;
use crate::daser::{DEFAULT_SAMPLING_WINDOW, MIN_SAMPLING_WINDOW};
use crate::events::EventSubscriber;
use crate::network::Network;
use crate::node::DEFAULT_SYNCING_WINDOW;
use crate::node::{Node, NodeConfig, Result};
use crate::pruner::{DEFAULT_PRUNING_DELAY, MIN_PRUNING_DELAY};
use crate::store::{InMemoryStore, Store};
use crate::syncer::MIN_SYNCING_WINDOW;

/// [`Node`] builder.
pub struct NodeBuilder<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    blockstore: B,
    store: S,
    keypair: Option<Keypair>,
    network: Option<Network>,
    bootnodes: Vec<Multiaddr>,
    listen: Vec<Multiaddr>,
    sync_batch_size: Option<u64>,
    syncing_window: Option<Duration>,
    sampling_window: Option<Duration>,
    pruning_delay: Option<Duration>,
}

impl NodeBuilder<InMemoryBlockstore, InMemoryStore> {
    pub fn new() -> Self {
        NodeBuilder {
            blockstore: InMemoryBlockstore::new(),
            store: InMemoryStore::new(),
            keypair: None,
            network: None,
            bootnodes: Vec::new(),
            listen: Vec::new(),
            sync_batch_size: None,
            syncing_window: None,
            sampling_window: None,
            pruning_delay: None,
        }
    }
}

impl<B, S> NodeBuilder<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    /// Set the [`Blockstore`] for Bitswap.
    ///
    /// **Default:** [`InMemoryBlockstore`]
    pub fn blockstore<B2>(self, blockstore: B2) -> NodeBuilder<B2, S>
    where
        B2: Blockstore + 'static,
    {
        NodeBuilder {
            blockstore,
            store: self.store,
            keypair: self.keypair,
            network: self.network,
            bootnodes: self.bootnodes,
            listen: self.listen,
            sync_batch_size: self.sync_batch_size,
            syncing_window: self.syncing_window,
            sampling_window: self.sampling_window,
            pruning_delay: self.pruning_delay,
        }
    }

    /// Set the [`Store`] for headers.
    ///
    /// **Default:** [`InMemoryStore`]
    pub fn store<S2>(self, store: S2) -> NodeBuilder<B, S2>
    where
        S2: Store + 'static,
    {
        NodeBuilder {
            blockstore: self.blockstore,
            store,
            keypair: self.keypair,
            network: self.network,
            bootnodes: self.bootnodes,
            listen: self.listen,
            sync_batch_size: self.sync_batch_size,
            syncing_window: self.syncing_window,
            sampling_window: self.sampling_window,
            pruning_delay: self.pruning_delay,
        }
    }

    /// The [`Network`] to connect to.
    pub fn network(self, network: Network) -> Self {
        NodeBuilder {
            network: Some(network),
            ..self
        }
    }

    /// Set the keypair to be used as [`Node`]s identity.
    ///
    /// **Default:** Random generated with [`Keypair::generate_ed25519`].
    pub fn keypair(self, keypair: Keypair) -> Self {
        NodeBuilder {
            keypair: Some(keypair),
            ..self
        }
    }

    /// Set the bootstrap nodes to connect and trust.
    ///
    /// **Default:** [`Network::canonical_bootnodes`]
    pub fn bootnodes<I>(self, addrs: I) -> Self
    where
        I: IntoIterator<Item = Multiaddr>,
    {
        NodeBuilder {
            bootnodes: addrs.into_iter().collect(),
            ..self
        }
    }

    /// Set the addresses where [`Node`] will listen for incoming connections.
    pub fn listen<I>(self, addrs: I) -> Self
    where
        I: IntoIterator<Item = Multiaddr>,
    {
        NodeBuilder {
            listen: addrs.into_iter().collect(),
            ..self
        }
    }

    /// Maximum number of headers in batch while syncing.
    ///
    /// **Default:** 512
    pub fn sync_batch_size(self, batch_size: u64) -> Self {
        NodeBuilder {
            sync_batch_size: Some(batch_size),
            ..self
        }
    }

    /// Set syncing window.
    ///
    /// Syncing window defines maximum age of headers considered for syncing.
    /// Headers older than syncing window by more than an hour are eligible for pruning.
    ///
    /// **Default if [`InMemoryStore`] is used:** 60 seconds.
    /// **Default:** 30 days.
    /// **Minimum:** 60 seconds.
    pub fn syncing_window(self, dur: Duration) -> Self {
        NodeBuilder {
            syncing_window: Some(dur),
            ..self
        }
    }

    /// Set sampling window.
    ///
    /// Sampling window defines the maximum age of a block considered for sampling.
    /// Sampling window will be truncated to syncing window, if latter is smaller.
    ///
    /// **Default if [`InMemoryBlockstore`] is used:** 60 seconds.
    /// **Default:** 30 days.
    /// **Minimum:** 60 seconds.
    pub fn sampling_window(self, dur: Duration) -> Self {
        NodeBuilder {
            sampling_window: Some(dur),
            ..self
        }
    }

    /// Set pruning delay.
    ///
    /// Pruning delay how much time the pruner should wait after syncing window in
    /// order to prune the block.
    ///
    /// **Default if [`InMemoryStore`] is used: 60 seconds.
    /// **Default:** 1 hour.
    /// **Minimum:** 60 seconds.
    pub fn pruning_delay(self, dur: Duration) -> Self {
        NodeBuilder {
            pruning_delay: Some(dur),
            ..self
        }
    }

    fn build_config(self) -> Result<NodeConfig<B, S>> {
        let network = self.network.expect("todo");

        let bootnodes = if self.bootnodes.is_empty() {
            network.canonical_bootnodes().collect()
        } else {
            self.bootnodes
        };

        if bootnodes.is_empty() && self.listen.is_empty() {
            // It is a valid scenario for user to create a node without any bootnodes
            // and listening addresses. However it may not be what they wanted. Because
            // of this we display a warning.
            warn!("Node has empty bootnodes and listening addresses. It will never connect to another peer.");
        }

        // `Node` is memory hungry when in-memory stores are used and the user most
        // probably will never know that they need to have a smaller syncing window
        // to avoid it. In order to make `lumina` more user friendly we decided to
        // have a different default when our in-memory stores are used.
        //
        // If user implements their own in-memory stores then they are responsible
        // to set the syncing window to something smaller than `DEFAULT_SYNCING_WINDOW`.
        let in_memory_store_used = TypeId::of::<S>() == TypeId::of::<InMemoryStore>();
        let in_memory_blockstore_used = TypeId::of::<B>() == TypeId::of::<InMemoryBlockstore>();

        let syncing_window = if let Some(dur) = self.syncing_window {
            dur
        } else if in_memory_store_used {
            MIN_SYNCING_WINDOW
        } else {
            DEFAULT_SYNCING_WINDOW
        };

        let sampling_window = if let Some(dur) = self.syncing_window {
            dur
        } else if in_memory_blockstore_used {
            MIN_SAMPLING_WINDOW
        } else {
            DEFAULT_SAMPLING_WINDOW
        };

        let pruning_delay = if let Some(dur) = self.pruning_delay {
            dur
        } else if in_memory_store_used {
            MIN_PRUNING_DELAY
        } else {
            DEFAULT_PRUNING_DELAY
        };

        if syncing_window < MIN_SYNCING_WINDOW {
            panic!("todo");
        }

        // Truncate sampling window if needed.
        let sampling_window = sampling_window.min(syncing_window);

        if sampling_window < MIN_SAMPLING_WINDOW {
            panic!("todo");
        }

        if pruning_delay < MIN_PRUNING_DELAY {
            panic!("todo");
        }

        let pruning_window = syncing_window.saturating_add(pruning_delay);

        info!(
            "Syncing window: {:?}, Sampling window: {:?}, Pruning window: {:?}",
            syncing_window, sampling_window, pruning_window
        );

        Ok(NodeConfig {
            blockstore: self.blockstore,
            store: self.store,
            network_id: network.id().to_owned(),
            p2p_local_keypair: self.keypair.unwrap_or_else(|| Keypair::generate_ed25519()),
            p2p_bootnodes: bootnodes,
            p2p_listen_on: self.listen,
            sync_batch_size: self.sync_batch_size.unwrap_or(512),
            syncing_window,
            sampling_window,
            pruning_window,
        })
    }

    pub async fn start(self) -> Result<Node<B, S>> {
        let config = self.build_config()?;
        Node::new(config).await
    }

    pub async fn start_subscribed(self) -> Result<(Node<B, S>, EventSubscriber)> {
        let config = self.build_config()?;
        Node::new_subscribed(config).await
    }
}
