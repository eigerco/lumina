//! A browser compatible wrappers for the [`lumina-node`].

use js_sys::Array;
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::to_value;
use tracing::error;
use wasm_bindgen::prelude::*;
use web_sys::BroadcastChannel;

use lumina_node::blockstore::IndexedDbBlockstore;
use lumina_node::network::{canonical_network_bootnodes, network_id};
use lumina_node::node::NodeConfig;
use lumina_node::store::IndexedDbStore;

use crate::commands::{CheckableResponseExt, NodeCommand, SingleHeaderQuery};
use crate::error::{Context, Result};
use crate::ports::RequestResponse;
use crate::utils::{
    is_safari, js_value_from_display, request_storage_persistence, resolve_dnsaddr_multiaddress,
    Network,
};
use crate::wrapper::libp2p::NetworkInfoSnapshot;

/// Config for the lumina wasm node.
#[wasm_bindgen(inspectable, js_name = NodeConfig)]
#[derive(Serialize, Deserialize, Debug)]
pub struct WasmNodeConfig {
    /// A network to connect to.
    pub network: Network,
    /// A list of bootstrap peers to connect to.
    #[wasm_bindgen(getter_with_clone)]
    pub bootnodes: Vec<String>,
}

/// `NodeDriver` is responsible for steering [`NodeWorker`] by sending it commands and receiving
/// responses over the provided port.
///
/// [`NodeWorker`]: crate::worker::NodeWorker
#[wasm_bindgen]
struct NodeClient {
    worker: RequestResponse,
}

#[wasm_bindgen]
impl NodeClient {
    /// Create a new connection to a Lumina node running in [`NodeWorker`]. Provided `port` is
    /// expected to have `MessagePort`-like interface for sending and receiving messages.
    pub async fn new(port: JsValue) -> Result<NodeClient> {
        // Safari doesn't have the `navigator.storage()` api
        if !is_safari()? {
            if let Err(e) = request_storage_persistence().await {
                error!("Error requesting storage persistence: {e}");
            }
        }

        Ok(Self {
            worker: RequestResponse::new(port)?,
        })
    }

    /// Check whether Lumina is currently running
    pub async fn is_running(&self) -> Result<bool> {
        let command = NodeCommand::IsRunning;
        let response = self.worker.exec(command).await?;
        let running = response.into_is_running().check_variant()?;

        Ok(running)
    }

    /// Start a node with the provided config, if it's not running
    pub async fn start(&self, config: WasmNodeConfig) -> Result<()> {
        let command = NodeCommand::StartNode(config);
        let response = self.worker.exec(command).await?;
        response.into_node_started().check_variant()??;

        Ok(())
    }

    /// Get node's local peer ID.
    pub async fn local_peer_id(&self) -> Result<String> {
        let command = NodeCommand::GetLocalPeerId;
        let response = self.worker.exec(command).await?;
        let peer_id = response.into_local_peer_id().check_variant()?;

        Ok(peer_id)
    }

    /// Get current [`PeerTracker`] info.
    pub async fn peer_tracker_info(&self) -> Result<JsValue> {
        let command = NodeCommand::GetPeerTrackerInfo;
        let response = self.worker.exec(command).await?;
        let peer_info = response.into_peer_tracker_info().check_variant()?;

        Ok(to_value(&peer_info)?)
    }

    /// Wait until the node is connected to at least 1 peer.
    pub async fn wait_connected(&self) -> Result<()> {
        let command = NodeCommand::WaitConnected { trusted: false };
        let response = self.worker.exec(command).await?;
        let _ = response.into_connected().check_variant()?;

        Ok(())
    }

    /// Wait until the node is connected to at least 1 trusted peer.
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        let command = NodeCommand::WaitConnected { trusted: true };
        let response = self.worker.exec(command).await?;
        response.into_connected().check_variant()?
    }

    /// Get current network info.
    pub async fn network_info(&self) -> Result<NetworkInfoSnapshot> {
        let command = NodeCommand::GetNetworkInfo;
        let response = self.worker.exec(command).await?;

        response.into_network_info().check_variant()?
    }

    /// Get all the multiaddresses on which the node listens.
    pub async fn listeners(&self) -> Result<Array> {
        let command = NodeCommand::GetListeners;
        let response = self.worker.exec(command).await?;
        let listeners = response.into_listeners().check_variant()?;
        let result = listeners?.iter().map(js_value_from_display).collect();

        Ok(result)
    }

    /// Get all the peers that node is connected to.
    pub async fn connected_peers(&self) -> Result<Array> {
        let command = NodeCommand::GetConnectedPeers;
        let response = self.worker.exec(command).await?;
        let peers = response.into_connected_peers().check_variant()?;
        let result = peers?.iter().map(js_value_from_display).collect();

        Ok(result)
    }

    /// Trust or untrust the peer with a given ID.
    pub async fn set_peer_trust(&self, peer_id: &str, is_trusted: bool) -> Result<()> {
        let command = NodeCommand::SetPeerTrust {
            peer_id: peer_id.parse()?,
            is_trusted,
        };
        let response = self.worker.exec(command).await?;
        response.into_set_peer_trust().check_variant()?
    }

    /// Request the head header from the network.
    pub async fn request_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::Head);
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request a header for the block with a given hash from the network.
    pub async fn request_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request a header for the block with a given height from the network.
    pub async fn request_header_by_height(&self, height: u64) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::ByHeight(height));
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request headers in range (from, from + amount] from the network.
    ///
    /// The headers will be verified with the `from` header.
    pub async fn request_verified_headers(
        &self,
        from_header: JsValue,
        amount: u64,
    ) -> Result<Array> {
        let command = NodeCommand::GetVerifiedHeaders {
            from: from_header,
            amount,
        };
        let response = self.worker.exec(command).await?;
        let headers = response.into_headers().check_variant()?;

        headers.into()
    }

    /// Get current header syncing info.
    pub async fn syncer_info(&self) -> Result<JsValue> {
        let command = NodeCommand::GetSyncerInfo;
        let response = self.worker.exec(command).await?;
        let syncer_info = response.into_syncer_info().check_variant()?;

        Ok(to_value(&syncer_info?)?)
    }

    /// Get the latest header announced in the network.
    pub async fn get_network_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::LastSeenNetworkHead;
        let response = self.worker.exec(command).await?;
        let header = response.into_last_seen_network_head().check_variant()?;

        header.into()
    }

    /// Get the latest locally synced header.
    pub async fn get_local_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::Head);
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Get a synced header for the block with a given hash.
    pub async fn get_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Get a synced header for the block with a given height.
    pub async fn get_header_by_height(&self, height: u64) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::ByHeight(height));
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Get synced headers from the given heights range.
    ///
    /// If start of the range is undefined (None), the first returned header will be of height 1.
    /// If end of the range is undefined (None), the last returned header will be the last header in the
    /// store.
    ///
    /// # Errors
    ///
    /// If range contains a height of a header that is not found in the store.
    pub async fn get_headers(
        &self,
        start_height: Option<u64>,
        end_height: Option<u64>,
    ) -> Result<Array> {
        let command = NodeCommand::GetHeadersRange {
            start_height,
            end_height,
        };
        let response = self.worker.exec(command).await?;
        let headers = response.into_headers().check_variant()?;

        headers.into()
    }

    /// Get data sampling metadata of an already sampled height.
    pub async fn get_sampling_metadata(&self, height: u64) -> Result<JsValue> {
        let command = NodeCommand::GetSamplingMetadata { height };
        let response = self.worker.exec(command).await?;
        let metadata = response.into_sampling_metadata().check_variant()?;

        Ok(to_value(&metadata?)?)
    }

    /// Requests SharedWorker running lumina to close. Any events received afterwards wont
    /// be processed and new NodeClient needs to be created to restart a node.
    pub async fn close(&self) -> Result<()> {
        let command = NodeCommand::CloseWorker;
        let response = self.worker.exec(command).await?;
        response.into_worker_closed().check_variant()?;

        Ok(())
    }

    /// Returns a [`BroadcastChannel`] for events generated by [`Node`].
    pub async fn events_channel(&self) -> Result<BroadcastChannel> {
        let command = NodeCommand::GetEventsChannelName;
        let response = self.worker.exec(command).await?;
        let name = response.into_events_channel_name().check_variant()?;

        Ok(BroadcastChannel::new(&name).unwrap())
    }
}

#[wasm_bindgen(js_class = NodeConfig)]
impl WasmNodeConfig {
    /// Get the configuration with default bootnodes for provided network
    pub fn default(network: Network) -> WasmNodeConfig {
        WasmNodeConfig {
            network,
            bootnodes: canonical_network_bootnodes(network.into())
                .map(|addr| addr.to_string())
                .collect::<Vec<_>>(),
        }
    }

    pub(crate) async fn into_node_config(
        self,
    ) -> Result<NodeConfig<IndexedDbBlockstore, IndexedDbStore>> {
        let network_id = network_id(self.network.into());
        let store = IndexedDbStore::new(network_id)
            .await
            .context("Failed to open the store")?;
        let blockstore = IndexedDbBlockstore::new(&format!("{network_id}-blockstore"))
            .await
            .context("Failed to open the blockstore")?;

        let p2p_local_keypair = Keypair::generate_ed25519();

        let mut p2p_bootnodes = Vec::with_capacity(self.bootnodes.len());
        for addr in self.bootnodes {
            let addr = addr
                .parse()
                .with_context(|| format!("invalid multiaddr: '{addr}"))?;
            let resolved_addrs = resolve_dnsaddr_multiaddress(addr).await?;
            p2p_bootnodes.extend(resolved_addrs.into_iter());
        }

        Ok(NodeConfig {
            network_id: network_id.to_string(),
            p2p_bootnodes,
            p2p_local_keypair,
            p2p_listen_on: vec![],
            sync_batch_size: 128,
            blockstore,
            store,
        })
    }
}
