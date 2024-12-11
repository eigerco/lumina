mod types;

use celestia_types::ExtendedHeader;
use libp2p::identity::Keypair;
use lumina_node::{
    blockstore::RedbBlockstore,
    events::{EventSubscriber, TryRecvError},
    network::{canonical_network_bootnodes, network_id},
    store::RedbStore,
    Node, NodeConfig, NodeError,
};
use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use tendermint::hash::Hash;
use thiserror::Error;
use tokio::sync::Mutex;
use types::{Network, NetworkInfo, NodeEvent, PeerId, PeerTrackerInfo, SyncingInfo};
use uniffi::Object;

pub type Result<T> = std::result::Result<T, LuminaError>;

/// Returns the platform-specific base path for storing Lumina data.
///
/// The function determines the base path based on the target operating system:
/// - **iOS**: `~/Library/Application Support/lumina`
/// - **Android**: Value of the `LUMINA_DATA_DIR` environment variable
/// - **Other platforms**: Returns an error indicating unsupported platform.
fn get_base_path() -> Result<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .map(|p| p.join("Library/Application Support/lumina"))
            .map_err(|e| LuminaError::StorageError {
                msg: format!("Could not get HOME directory: {}", e),
            })
    }

    #[cfg(target_os = "android")]
    {
        std::env::var("LUMINA_DATA_DIR")
            .map(PathBuf::from)
            .map_err(|e| LuminaError::StorageError {
                msg: format!("Could not get LUMINA_DATA_DIR: {}", e),
            })
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        Err(LuminaError::StorageError {
            msg: "Unsupported platform".to_string(),
        })
    }
}

#[derive(Error, Debug, uniffi::Error)]
pub enum LuminaError {
    #[error("Node is not running")]
    NodeNotRunning,
    #[error("Network error: {msg}")]
    NetworkError { msg: String },
    #[error("Storage error: {msg}")]
    StorageError { msg: String },
    #[error("Node is already running")]
    AlreadyRunning,
    #[error("Lock error")]
    LockError,
    #[error("Invalid hash format: {msg}")]
    InvalidHash { msg: String },
    #[error("Invalid header format: {msg}")]
    InvalidHeader { msg: String },
    #[error("Storage initialization failed: {msg}")]
    StorageInit { msg: String },
}

impl From<NodeError> for LuminaError {
    fn from(error: NodeError) -> Self {
        LuminaError::NetworkError {
            msg: error.to_string(),
        }
    }
}

/// The main Lumina node that manages the connection to the Celestia network.
#[derive(Object)]
pub struct LuminaNode {
    node: Arc<Mutex<Option<Node<RedbBlockstore, RedbStore>>>>,
    network: Network,
    events_subscriber: Arc<Mutex<Option<EventSubscriber>>>,
}

#[uniffi::export(async_runtime = "tokio")]
impl LuminaNode {
    /// Sets a new connection to the Lumina node for the specified network.
    #[uniffi::constructor]
    pub fn new(network: Network) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            node: Arc::new(Mutex::new(None)),
            network,
            events_subscriber: Arc::new(Mutex::new(None)),
        }))
    }

    /// Starts the Lumina node. Returns true if successfully started.
    pub async fn start(&self) -> Result<bool> {
        let mut node_guard = self.node.lock().await;

        if node_guard.is_some() {
            return Err(LuminaError::AlreadyRunning);
        }

        let network_id = network_id(self.network.into());

        let base_path = get_base_path()?;

        std::fs::create_dir_all(&base_path).map_err(|e| LuminaError::StorageError {
            msg: format!("Failed to create data directory: {}", e),
        })?;

        let store_path = base_path.join(format!("store-{}", network_id));
        let db = Arc::new(redb::Database::create(&store_path).map_err(|e| {
            LuminaError::StorageInit {
                msg: format!("Failed to create database: {}", e),
            }
        })?);

        let store = RedbStore::new(db.clone())
            .await
            .map_err(|e| LuminaError::StorageInit {
                msg: format!("Failed to initialize store: {}", e),
            })?;

        let blockstore = RedbBlockstore::new(db);

        let p2p_bootnodes = canonical_network_bootnodes(self.network.into()).collect::<Vec<_>>();

        let p2p_local_keypair = Keypair::generate_ed25519();

        let config = NodeConfig {
            network_id: network_id.to_string(),
            p2p_bootnodes,
            p2p_local_keypair,
            p2p_listen_on: vec![],
            sync_batch_size: 128,
            custom_syncing_window: Some(Duration::from_secs(60 * 60 * 24)),
            store,
            blockstore,
        };

        let new_node = Node::new(config)
            .await
            .map_err(|e| LuminaError::NetworkError { msg: e.to_string() })?;

        let subscriber = new_node.event_subscriber();
        let mut events_guard = self.events_subscriber.lock().await;
        *events_guard = Some(subscriber);

        *node_guard = Some(new_node);
        Ok(true)
    }

    /// Stops the running node and closes all network connections.
    pub async fn stop(&self) -> Result<()> {
        let mut node_guard = self.node.lock().await;
        let node = node_guard.take().ok_or(LuminaError::NodeNotRunning)?;
        node.stop().await;
        Ok(())
    }

    /// Checks if the node is currently running.
    pub async fn is_running(&self) -> bool {
        self.node.lock().await.is_some()
    }

    /// Gets the local peer ID as a string.
    pub async fn local_peer_id(&self) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        Ok(node.local_peer_id().to_base58())
    }

    /// Gets information about connected peers.
    pub async fn peer_tracker_info(&self) -> Result<PeerTrackerInfo> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        Ok(node.peer_tracker_info().into())
    }

    /// Waits until the node is connected to at least one peer.
    pub async fn wait_connected(&self) -> Result<()> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        Ok(node.wait_connected().await?)
    }

    /// Waits until the node is connected to at least one trusted peer.
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        Ok(node.wait_connected_trusted().await?)
    }

    /// Gets current network information.
    pub async fn network_info(&self) -> Result<NetworkInfo> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let info = node.network_info().await?;
        Ok(info.into())
    }

    /// Gets list of addresses the node is listening to.
    pub async fn listeners(&self) -> Result<Vec<String>> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let listeners = node.listeners().await?;
        Ok(listeners.into_iter().map(|l| l.to_string()).collect())
    }

    /// Gets list of currently connected peer IDs.
    pub async fn connected_peers(&self) -> Result<Vec<PeerId>> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let peers = node.connected_peers().await?;
        Ok(peers.into_iter().map(PeerId::from).collect())
    }

    /// Sets whether a peer with give ID is trusted.
    pub async fn set_peer_trust(&self, peer_id: PeerId, is_trusted: bool) -> Result<()> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let peer_id = peer_id
            .to_libp2p()
            .map_err(|e| LuminaError::NetworkError { msg: e })?;
        Ok(node.set_peer_trust(peer_id, is_trusted).await?)
    }

    /// Request the head header from the network.
    ///
    /// Returns a serialized ExtendedHeader string.
    pub async fn request_head_header(&self) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let header = node.request_head_header().await?;
        Ok(header.to_string()) //if extended header is needed, we need a wrapper
    }

    /// Request a header for the block with a given hash from the network.
    pub async fn request_header_by_hash(&self, hash: String) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let hash =
            Hash::from_str(&hash).map_err(|e| LuminaError::InvalidHash { msg: e.to_string() })?;
        let header = node.request_header_by_hash(&hash).await?;
        Ok(header.to_string()) //if extended header is needed, we need a wrapper
    }

    /// Requests a header by its height.
    pub async fn request_header_by_height(&self, height: u64) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let header = node.request_header_by_height(height).await?;
        Ok(header.to_string())
    }

    /// Request headers in range (from, from + amount] from the network.
    ///
    /// The headers will be verified with the `from` header.
    /// Returns array of serialized ExtendedHeader strings.
    pub async fn request_verified_headers(
        &self,
        from: String, // serialized header like its done for WASM
        amount: u64,
    ) -> Result<Vec<String>> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let from: ExtendedHeader =
            serde_json::from_str(&from).map_err(|e| LuminaError::InvalidHeader {
                msg: format!("Invalid header JSON: {}", e),
            })?;
        let headers = node.request_verified_headers(&from, amount).await?;
        Ok(headers.into_iter().map(|h| h.to_string()).collect())
    }

    /// Gets current syncing information.
    pub async fn syncer_info(&self) -> Result<SyncingInfo> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let info = node.syncer_info().await?;
        Ok(info.into())
    }

    /// Gets the latest header announced in the network.
    pub async fn get_network_head_header(&self) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let header = node.get_network_head_header().await?;
        header.map_or(
            // todo: better error handling, its undefined in wasm
            Err(LuminaError::NetworkError {
                msg: "No network head header available".to_string(),
            }),
            |h| Ok(h.to_string()),
        )
    }

    /// Gets the latest locally synced header.
    pub async fn get_local_head_header(&self) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let header = node.get_local_head_header().await?;
        Ok(header.to_string())
    }

    /// Get a synced header for the block with a given hash.
    pub async fn get_header_by_hash(&self, hash: String) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let hash =
            Hash::from_str(&hash).map_err(|e| LuminaError::InvalidHash { msg: e.to_string() })?;
        let header = node.get_header_by_hash(&hash).await?;
        Ok(header.to_string())
    }

    /// Get a synced header for the block with a given height.
    pub async fn get_header_by_height(&self, height: u64) -> Result<String> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let header = node.get_header_by_height(height).await?;
        Ok(header.to_string())
    }

    /// Gets headers from the given heights range.
    ///
    /// If start of the range is undefined (None), the first returned header will be of height 1.
    /// If end of the range is undefined (None), the last returned header will be the last header in the
    /// store.
    ///
    /// Returns array of serialized ExtendedHeader strings.
    pub async fn get_headers(
        &self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<Vec<String>> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;

        let headers = match (start_height, end_height) {
            (None, None) => node.get_headers(..).await,
            (Some(start), None) => node.get_headers(start..).await,
            (None, Some(end)) => node.get_headers(..=end).await,
            (Some(start), Some(end)) => node.get_headers(start..=end).await,
        }?;

        Ok(headers.into_iter().map(|h| h.to_string()).collect())
    }

    /// Gets data sampling metadata for a height.
    ///
    /// Returns serialized SamplingMetadata string if metadata exists for the height.
    pub async fn get_sampling_metadata(&self, height: u64) -> Result<Option<String>> {
        let node_guard = self.node.lock().await;
        let node = node_guard.as_ref().ok_or(LuminaError::NodeNotRunning)?;
        let metadata = node.get_sampling_metadata(height).await?;
        Ok(metadata.map(|m| serde_json::to_string(&m).unwrap()))
    }

    /// Returns the next event from the node's event channel.
    pub async fn events_channel(&self) -> Result<Option<NodeEvent>> {
        let mut events_guard = self.events_subscriber.lock().await;
        let subscriber = events_guard.as_mut().ok_or(LuminaError::NodeNotRunning)?;

        match subscriber.try_recv() {
            Ok(event) => Ok(Some(event.event.into())),
            Err(TryRecvError::Empty) => Ok(None),
            Err(e) => Err(LuminaError::NetworkError { msg: e.to_string() }),
        }
    }
}

uniffi::setup_scaffolding!();
