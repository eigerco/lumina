use std::any::TypeId;
use std::time::Duration;

use blockstore::Blockstore;
use libp2p::identity::Keypair;
use libp2p::Multiaddr;
use tracing::{info, warn};

use crate::blockstore::InMemoryBlockstore;
use crate::events::EventSubscriber;
use crate::network::Network;
use crate::node::{Node, NodeConfig, Result};
use crate::store::{InMemoryStore, Store};

const HOUR: u64 = 60 * 60;
const DAY: u64 = 24 * HOUR;

/// Default maximum age of blocks [`Node`] will synchronise, sample, and store.
pub const DEFAULT_SYNCING_WINDOW: Duration = Duration::from_secs(30 * DAY);
/// Minimum configurable syncing window that can be used in [`NodeBuilder`].
pub const MIN_SYNCING_WINDOW: Duration = Duration::from_secs(60);

/// Default delay after the syncing window before [`Node`] prunes the block.
pub const DEFAULT_PRUNING_DELAY: Duration = Duration::from_secs(HOUR);
/// Minimum pruning delay that can be used in [`NodeBuilder`].
pub const MIN_PRUNING_DELAY: Duration = Duration::from_secs(60);

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
    pruning_delay: Option<Duration>,
}

/// Representation of all the errors that can occur when interacting with the [`NodeBuilder`].
#[derive(Debug, thiserror::Error)]
pub enum NodeBuilderError {
    /// Network is not specified
    #[error("Network is not specified")]
    NetworkNotSpecified,

    /// Syncing window is smaller than [`MIN_SYNCING_WINDOW`].
    #[error("Syncing window is {0:?} but cannot be smaller than {MIN_SYNCING_WINDOW:?}")]
    SyncingWindowTooSmall(Duration),

    /// Pruning delay is smaller than [`MIN_PRUNING_DELAY`].
    #[error("Pruning delay is {0:?} but cannot be smaller than {MIN_PRUNING_DELAY:?}")]
    PruningDelayTooSmall(Duration),
}

impl NodeBuilder<InMemoryBlockstore, InMemoryStore> {
    /// Creates a new [`NodeBuilder`] which uses in-memory stores.
    ///
    /// After the creation you can call [`NodeBuilder::blockstore`]
    /// and [`NodeBuilder::store`] to use other stores.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use lumina_node::network::Network;
    /// # use lumina_node::NodeBuilder;
    /// #
    /// # async fn example() {
    /// let node = NodeBuilder::new()
    ///     .network(Network::Mainnet)
    ///     .start()
    ///     .await
    ///     .unwrap();
    /// # }
    /// ```
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
            pruning_delay: None,
        }
    }
}

impl Default for NodeBuilder<InMemoryBlockstore, InMemoryStore> {
    fn default() -> Self {
        NodeBuilder::new()
    }
}

impl<B, S> NodeBuilder<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    /// Creates and starts a new Celestia [`Node`].
    pub async fn start(self) -> Result<Node<B, S>> {
        let (node, _) = self.start_subscribed().await?;
        Ok(node)
    }

    /// Creates and starts a new Celestia [`Node`].
    ///
    /// Returns [`Node`] along with [`EventSubscriber`]. Use this to avoid missing
    /// any events that will be generated on the construction of the node.
    pub async fn start_subscribed(self) -> Result<(Node<B, S>, EventSubscriber)> {
        let config = self.build_config()?;
        Node::start(config).await
    }

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
    /// Syncing window defines maximum age of a block considered for syncing and sampling.
    ///
    /// **Default if [`InMemoryStore`]/[`InMemoryBlockstore`] are used:** 60 seconds.\
    /// **Default:** 30 days.\
    /// **Minimum:** 60 seconds.
    pub fn syncing_window(self, dur: Duration) -> Self {
        NodeBuilder {
            syncing_window: Some(dur),
            ..self
        }
    }

    /// Set pruning delay.
    ///
    /// Pruning delay defines how much time the pruner should wait after syncing window in
    /// order to prune the block.
    ///
    /// **Default if [`InMemoryStore`]/[`InMemoryBlockstore`] are used:** 60 seconds.\
    /// **Default:** 1 hour.\
    /// **Minimum:** 60 seconds.
    pub fn pruning_delay(self, dur: Duration) -> Self {
        NodeBuilder {
            pruning_delay: Some(dur),
            ..self
        }
    }

    fn build_config(self) -> Result<NodeConfig<B, S>, NodeBuilderError> {
        let network = self.network.ok_or(NodeBuilderError::NetworkNotSpecified)?;

        let bootnodes = if self.bootnodes.is_empty() {
            network.canonical_bootnodes().collect()
        } else {
            self.bootnodes
        };

        if bootnodes.is_empty() && self.listen.is_empty() {
            // It is a valid scenario for user to create a node without any bootnodes
            // and listening addresses. However it may not be what they wanted. Because
            // of that we display a warning.
            warn!("Node has empty bootnodes and listening addresses. It will never connect to another peer.");
        }

        // `Node` is memory hungry when in-memory stores are used and the user may not
        // expect they should set a smaller syncing window to reduce that. For user-friendliness
        // sake, use smaller default syncing window, if we're running in memory.
        //
        // If user implements their own in-memory stores then they are responsible
        // to set the syncing window to something smaller than `DEFAULT_SYNCING_WINDOW`.
        let in_memory_stores_used = TypeId::of::<S>() == TypeId::of::<InMemoryStore>()
            || TypeId::of::<B>() == TypeId::of::<InMemoryBlockstore>();

        let syncing_window = if let Some(dur) = self.syncing_window {
            dur
        } else if in_memory_stores_used {
            MIN_SYNCING_WINDOW
        } else {
            DEFAULT_SYNCING_WINDOW
        };

        let pruning_delay = if let Some(dur) = self.pruning_delay {
            dur
        } else if in_memory_stores_used {
            MIN_PRUNING_DELAY
        } else {
            DEFAULT_PRUNING_DELAY
        };

        if syncing_window < MIN_SYNCING_WINDOW {
            return Err(NodeBuilderError::SyncingWindowTooSmall(syncing_window));
        }

        if pruning_delay < MIN_PRUNING_DELAY {
            return Err(NodeBuilderError::PruningDelayTooSmall(pruning_delay));
        }

        let pruning_window = syncing_window.saturating_add(pruning_delay);

        info!("Syncing window: {syncing_window:?}, Pruning window: {pruning_window:?}",);

        Ok(NodeConfig {
            blockstore: self.blockstore,
            store: self.store,
            network_id: network.id().to_owned(),
            p2p_local_keypair: self.keypair.unwrap_or_else(Keypair::generate_ed25519),
            p2p_bootnodes: bootnodes,
            p2p_listen_on: self.listen,
            sync_batch_size: self.sync_batch_size.unwrap_or(512),
            syncing_window,
            pruning_window,
        })
    }
}
