//! Native library providing Rust to mobile language bindings for the Lumina node.
//!
//! This crate uses Mozillas UniFFI to generate Swift and Kotlin bindings for the Lumina node,
//! allowing it to be used from iOS and Android applications.
#![cfg(not(target_arch = "wasm32"))]

mod error;
mod types;

use celestia_types::ExtendedHeader;
use error::{LuminaError, Result};
use lumina_node::{
    blockstore::RedbBlockstore,
    events::{EventSubscriber, TryRecvError},
    network::Network,
    node::PeerTrackerInfo,
    store::RedbStore,
    Node,
};
use std::{str::FromStr, sync::Arc};
use tendermint::hash::Hash;
use tokio::sync::Mutex;
use types::{NetworkInfo, NodeEvent, NodeStartConfig, PeerId, SyncingInfo};
use uniffi::Object;

uniffi::setup_scaffolding!();

lumina_node::uniffi_reexport_scaffolding!();

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

    /// Start the node without optional configuration.
    /// UniFFI needs explicit handling for optional parameters to generate correct bindings for different languages.
    pub async fn start(&self) -> Result<bool> {
        self.start_with_config(None).await
    }

    /// Start the node with specific configuration
    pub async fn start_with_config(&self, config: Option<NodeStartConfig>) -> Result<bool> {
        let mut node_guard = self.node.lock().await;

        if node_guard.is_some() {
            return Err(LuminaError::AlreadyRunning);
        }

        let config = config.unwrap_or_else(|| NodeStartConfig {
            network: self.network.clone(),
            bootnodes: None,
            syncing_window_secs: None,
            pruning_delay_secs: None,
            batch_size: None,
            ed25519_secret_key_bytes: None,
        });

        let builder = config.into_node_builder().await?;

        let (new_node, subscriber) = builder
            .start_subscribed()
            .await
            .map_err(|e| LuminaError::NetworkError { msg: e.to_string() })?;

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
        Ok(node.peer_tracker_info())
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
            .map_err(|e| LuminaError::NetworkError { msg: e.to_string() })?;
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
