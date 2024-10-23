//! Node that connects to Celestia's P2P network.
//!
//! Upon creation, `Node` will try to connect to Celestia's P2P network
//! and then proceed with synchronization and data sampling of the blocks.

use std::ops::RangeBounds;
use std::sync::Arc;
use std::time::Duration;

use blockstore::Blockstore;
use celestia_types::hash::Hash;
use celestia_types::nmt::Namespace;
use celestia_types::row::Row;
use celestia_types::row_namespace_data::RowNamespaceData;
use celestia_types::sample::Sample;
use celestia_types::ExtendedHeader;
use libp2p::identity::Keypair;
use libp2p::swarm::NetworkInfo;
use libp2p::{Multiaddr, PeerId};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::daser::{Daser, DaserArgs};
use crate::events::{EventChannel, EventSubscriber, NodeEvent};
use crate::executor::{spawn_cancellable, JoinHandle};
use crate::p2p::{P2p, P2pArgs};
use crate::pruner::{Pruner, PrunerArgs, DEFAULT_PRUNING_INTERVAL};
use crate::store::{SamplingMetadata, Store, StoreError};
use crate::syncer::{Syncer, SyncerArgs};

pub use crate::daser::DaserError;
pub use crate::p2p::{HeaderExError, P2pError};
pub use crate::peer_tracker::PeerTrackerInfo;
pub use crate::syncer::{SyncerError, SyncingInfo, DEFAULT_SYNCING_WINDOW};

/// Alias of [`Result`] with [`NodeError`] error type
///
/// [`Result`]: std::result::Result
pub type Result<T, E = NodeError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with the [`Node`].
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    /// An error propagated from the `P2p` component.
    #[error("P2p: {0}")]
    P2p(#[from] P2pError),

    /// An error propagated from the `Syncer` component.
    #[error("Syncer: {0}")]
    Syncer(#[from] SyncerError),

    /// An error propagated from the [`Store`] component.
    #[error("Store: {0}")]
    Store(#[from] StoreError),

    /// An error propagated from the `Daser` component.
    #[error("Daser: {0}")]
    Daser(#[from] DaserError),
}

/// Node conifguration.
pub struct NodeConfig<B, S>
where
    B: Blockstore,
    S: Store,
{
    /// An id of the network to connect to.
    pub network_id: String,
    /// The keypair to be used as [`Node`]s identity.
    pub p2p_local_keypair: Keypair,
    /// List of bootstrap nodes to connect to and trust.
    pub p2p_bootnodes: Vec<Multiaddr>,
    /// List of the addresses where [`Node`] will listen for incoming connections.
    pub p2p_listen_on: Vec<Multiaddr>,
    /// Maximum number of headers in batch while syncing.
    pub sync_batch_size: u64,
    /// Syncing window size, defines maximum age of headers considered for syncing and sampling.
    /// Headers older than syncing window by more than an hour are eligible for pruning.
    pub custom_syncing_window: Option<Duration>,
    /// The blockstore for bitswap.
    pub blockstore: B,
    /// The store for headers.
    pub store: S,
}

/// Celestia node.
pub struct Node<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    event_channel: EventChannel,
    p2p: Option<Arc<P2p>>,
    blockstore: Option<Arc<B>>,
    store: Option<Arc<S>>,
    syncer: Option<Arc<Syncer<S>>>,
    daser: Option<Arc<Daser>>,
    pruner: Option<Arc<Pruner>>,
    tasks_cancellation_token: CancellationToken,
    network_compromised_task: JoinHandle,
}

impl<B, S> Node<B, S>
where
    B: Blockstore,
    S: Store,
{
    /// Creates and starts a new celestia node with a given config.
    pub async fn new(config: NodeConfig<B, S>) -> Result<Self> {
        let (node, _) = Node::new_subscribed(config).await?;
        Ok(node)
    }

    /// Creates and starts a new celestia node with a given config.
    ///
    /// Returns `Node` alogn with `EventSubscriber`. Use this to avoid missing any
    /// events that will be generated on the construction of the node.
    pub async fn new_subscribed(config: NodeConfig<B, S>) -> Result<(Self, EventSubscriber)> {
        let event_channel = EventChannel::new();
        let event_sub = event_channel.subscribe();
        let store = Arc::new(config.store);
        let blockstore = Arc::new(config.blockstore);
        let syncing_window = config
            .custom_syncing_window
            .unwrap_or(DEFAULT_SYNCING_WINDOW);

        let p2p = Arc::new(
            P2p::start(P2pArgs {
                network_id: config.network_id,
                local_keypair: config.p2p_local_keypair,
                bootnodes: config.p2p_bootnodes,
                listen_on: config.p2p_listen_on,
                blockstore: blockstore.clone(),
                store: store.clone(),
                event_pub: event_channel.publisher(),
            })
            .await?,
        );

        let syncer = Arc::new(Syncer::start(SyncerArgs {
            store: store.clone(),
            p2p: p2p.clone(),
            event_pub: event_channel.publisher(),
            batch_size: config.sync_batch_size,
            syncing_window,
        })?);

        let daser = Arc::new(Daser::start(DaserArgs {
            p2p: p2p.clone(),
            store: store.clone(),
            event_pub: event_channel.publisher(),
            syncing_window,
        })?);

        let pruner = Arc::new(Pruner::start(PrunerArgs {
            store: store.clone(),
            blockstore: blockstore.clone(),
            event_pub: event_channel.publisher(),
            pruning_interval: DEFAULT_PRUNING_INTERVAL,
            syncing_window,
        }));

        let tasks_cancellation_token = CancellationToken::new();

        // spawn the task that will stop the services when the fraud is detected
        let network_compromised_task = spawn_cancellable(tasks_cancellation_token.child_token(), {
            let network_compromised_token = p2p.get_network_compromised_token().await?;
            let syncer = syncer.clone();
            let daser = daser.clone();
            let pruner = pruner.clone();
            let event_pub = event_channel.publisher();

            async move {
                network_compromised_token.triggered().await;

                // Network compromised! Stop workers.
                syncer.stop();
                daser.stop();
                pruner.stop();

                event_pub.send(NodeEvent::NetworkCompromised);
                // This is a very important message and we want to log it even
                // if user consumes our events.
                warn!("{}", NodeEvent::NetworkCompromised);
            }
        });

        let node = Node {
            event_channel,
            p2p: Some(p2p),
            blockstore: Some(blockstore),
            store: Some(store),
            syncer: Some(syncer),
            daser: Some(daser),
            pruner: Some(pruner),
            tasks_cancellation_token,
            network_compromised_task,
        };

        Ok((node, event_sub))
    }

    /// Stop the node.
    pub async fn stop(mut self) {
        {
            let daser = self.daser.take().expect("Daser not initialized");
            let syncer = self.syncer.take().expect("Syncer not initialized");
            let pruner = self.pruner.take().expect("Pruner not initialized");
            let p2p = self.p2p.take().expect("P2p not initialized");

            // Cancel Node's tasks
            self.tasks_cancellation_token.cancel();
            self.network_compromised_task.join().await;

            // Stop all components that use P2p.
            daser.stop();
            syncer.stop();
            pruner.stop();

            daser.join().await;
            syncer.join().await;
            pruner.join().await;

            // Now stop P2p component.
            p2p.stop();
            p2p.join().await;
        }

        // Everything that was holding Blockstore is now dropped, so we can close it.
        let blockstore = self.blockstore.take().expect("Blockstore not initialized");
        let blockstore = Arc::into_inner(blockstore).expect("Not all Arc<Blockstore> were dropped");
        if let Err(e) = blockstore.close().await {
            warn!("Blockstore failed to close: {e}");
        }

        // Everything that was holding Store is now dropped, so we can close it.
        let store = self.store.take().expect("Store not initialized");
        let store = Arc::into_inner(store).expect("Not all Arc<Store> were dropped");
        if let Err(e) = store.close().await {
            warn!("Store failed to close: {e}");
        }

        self.event_channel.publisher().send(NodeEvent::NodeStopped);
    }

    fn syncer(&self) -> &Syncer<S> {
        self.syncer.as_ref().expect("Syncer not initialized")
    }

    fn p2p(&self) -> &P2p {
        self.p2p.as_ref().expect("P2p not initialized")
    }

    fn store(&self) -> &S {
        self.store.as_ref().expect("Store not initialized")
    }

    /// Returns a new `EventSubscriber`.
    pub fn event_subscriber(&self) -> EventSubscriber {
        self.event_channel.subscribe()
    }

    /// Get node's local peer ID.
    pub fn local_peer_id(&self) -> &PeerId {
        self.p2p().local_peer_id()
    }

    /// Get current [`PeerTrackerInfo`].
    pub fn peer_tracker_info(&self) -> PeerTrackerInfo {
        self.p2p().peer_tracker_info().clone()
    }

    /// Get [`PeerTrackerInfo`] watcher.
    pub fn peer_tracker_info_watcher(&self) -> watch::Receiver<PeerTrackerInfo> {
        self.p2p().peer_tracker_info_watcher()
    }

    /// Wait until the node is connected to at least 1 peer.
    pub async fn wait_connected(&self) -> Result<()> {
        Ok(self.p2p().wait_connected().await?)
    }

    /// Wait until the node is connected to at least 1 trusted peer.
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        Ok(self.p2p().wait_connected_trusted().await?)
    }

    /// Get current network info.
    pub async fn network_info(&self) -> Result<NetworkInfo> {
        Ok(self.p2p().network_info().await?)
    }

    /// Get all the multiaddresses on which the node listens.
    pub async fn listeners(&self) -> Result<Vec<Multiaddr>> {
        Ok(self.p2p().listeners().await?)
    }

    /// Get all the peers that node is connected to.
    pub async fn connected_peers(&self) -> Result<Vec<PeerId>> {
        Ok(self.p2p().connected_peers().await?)
    }

    /// Trust or untrust the peer with a given ID.
    pub async fn set_peer_trust(&self, peer_id: PeerId, is_trusted: bool) -> Result<()> {
        Ok(self.p2p().set_peer_trust(peer_id, is_trusted).await?)
    }

    /// Request the head header from the network.
    pub async fn request_head_header(&self) -> Result<ExtendedHeader> {
        Ok(self.p2p().get_head_header().await?)
    }

    /// Request a header for the block with a given hash from the network.
    pub async fn request_header_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        Ok(self.p2p().get_header(*hash).await?)
    }

    /// Request a header for the block with a given height from the network.
    pub async fn request_header_by_height(&self, hash: u64) -> Result<ExtendedHeader> {
        Ok(self.p2p().get_header_by_height(hash).await?)
    }

    /// Request headers in range (from, from + amount] from the network.
    ///
    /// The headers will be verified with the `from` header.
    pub async fn request_verified_headers(
        &self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        Ok(self.p2p().get_verified_headers_range(from, amount).await?)
    }

    /// Request a verified [`Row`] from the network.
    ///
    /// # Errors
    ///
    /// On failure to receive a verified [`Row`] within a certain time, the
    /// `NodeError::P2p(P2pError::BitswapQueryTimeout)` error will be returned.
    pub async fn request_row(&self, row_index: u16, block_height: u64) -> Result<Row> {
        Ok(self.p2p().get_row(row_index, block_height).await?)
    }

    /// Request a verified [`Sample`] from the network.
    ///
    /// # Errors
    ///
    /// On failure to receive a verified [`Sample`] within a certain time, the
    /// `NodeError::P2p(P2pError::BitswapQueryTimeout)` error will be returned.
    pub async fn request_sample(
        &self,
        row_index: u16,
        column_index: u16,
        block_height: u64,
    ) -> Result<Sample> {
        Ok(self
            .p2p()
            .get_sample(row_index, column_index, block_height)
            .await?)
    }

    /// Request a verified [`RowNamespaceData`] from the network.
    ///
    /// # Errors
    ///
    /// On failure to receive a verified [`RowNamespaceData`] within a certain time, the
    /// `NodeError::P2p(P2pError::BitswapQueryTimeout)` error will be returned.
    pub async fn request_row_namespace_data(
        &self,
        namespace: Namespace,
        row_index: u16,
        block_height: u64,
    ) -> Result<RowNamespaceData> {
        Ok(self
            .p2p()
            .get_row_namespace_data(namespace, row_index, block_height)
            .await?)
    }

    /// Get current header syncing info.
    pub async fn syncer_info(&self) -> Result<SyncingInfo> {
        Ok(self.syncer().info().await?)
    }

    /// Get the latest header announced in the network.
    pub async fn get_network_head_header(&self) -> Result<Option<ExtendedHeader>> {
        Ok(self.p2p().get_network_head().await?)
    }

    /// Get the latest locally synced header.
    pub async fn get_local_head_header(&self) -> Result<ExtendedHeader> {
        Ok(self.store().get_head().await?)
    }

    /// Get a synced header for the block with a given hash.
    pub async fn get_header_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader> {
        Ok(self.store().get_by_hash(hash).await?)
    }

    /// Get a synced header for the block with a given height.
    pub async fn get_header_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        Ok(self.store().get_by_height(height).await?)
    }

    /// Get synced headers from the given heights range.
    ///
    /// If start of the range is unbounded, the first returned header will be of height 1.
    /// If end of the range is unbounded, the last returned header will be the last header in the
    /// store.
    ///
    /// # Errors
    ///
    /// If range contains a height of a header that is not found in the store or [`RangeBounds`]
    /// cannot be converted to a valid range.
    pub async fn get_headers<R>(&self, range: R) -> Result<Vec<ExtendedHeader>>
    where
        R: RangeBounds<u64> + Send,
    {
        Ok(self.store().get_range(range).await?)
    }

    /// Get data sampling metadata of an already sampled height.
    ///
    /// Returns `Ok(None)` if metadata for the given height does not exists.
    pub async fn get_sampling_metadata(&self, height: u64) -> Result<Option<SamplingMetadata>> {
        match self.store().get_sampling_metadata(height).await {
            Ok(val) => Ok(val),
            Err(StoreError::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

impl<B, S> Drop for Node<B, S>
where
    B: Blockstore,
    S: Store,
{
    fn drop(&mut self) {
        // Stop everything, but don't join them.
        self.tasks_cancellation_token.cancel();

        if let Some(daser) = self.daser.take() {
            daser.stop();
        }

        if let Some(syncer) = self.syncer.take() {
            syncer.stop();
        }

        if let Some(pruner) = self.pruner.take() {
            pruner.stop();
        }

        if let Some(p2p) = self.p2p.take() {
            p2p.stop();
        }
    }
}
