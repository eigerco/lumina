//! A browser compatible wrappers for the [`lumina-node`].

use js_sys::Array;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;
use web_sys::BroadcastChannel;

use lumina_node::blockstore::IndexedDbBlockstore;
use lumina_node::network::{canonical_network_bootnodes, network_id};
use lumina_node::node::NodeConfig;
use lumina_node::store::IndexedDbStore;

use crate::error::{Context, Result};
use crate::utils::{is_chrome, js_value_from_display, Network};
use crate::worker::commands::{CheckableResponseExt, NodeCommand, SingleHeaderQuery};
use crate::worker::{AnyWorker, WorkerClient};
use crate::wrapper::libp2p::NetworkInfoSnapshot;

const LUMINA_WORKER_NAME: &str = "lumina";

/// Config for the lumina wasm node.
#[wasm_bindgen(js_name = NodeConfig)]
#[derive(Serialize, Deserialize, Debug)]
pub struct WasmNodeConfig {
    /// A network to connect to.
    pub network: Network,
    /// A list of bootstrap peers to connect to.
    #[wasm_bindgen(getter_with_clone)]
    pub bootnodes: Vec<String>,
}

/// `NodeDriver` represents lumina node running in a dedicated Worker/SharedWorker.
/// It's responsible for sending commands and receiving responses from the node.
#[wasm_bindgen(js_name = NodeClient)]
struct NodeDriver {
    client: WorkerClient,
}

/// Type of worker to run lumina in. Allows overriding automatically detected worker kind
/// (which should usually be appropriate).
#[wasm_bindgen]
pub enum NodeWorkerKind {
    /// Run in [`SharedWorker`]
    ///
    /// [`SharedWorker`]: https://developer.mozilla.org/en-US/docs/Web/API/SharedWorker
    Shared,
    /// Run in [`Worker`]
    ///
    /// [`Worker`]: https://developer.mozilla.org/en-US/docs/Web/API/Worker
    Dedicated,
}

#[wasm_bindgen(js_class = NodeClient)]
impl NodeDriver {
    /// Create a new connection to a Lumina node running in a Shared Worker.
    /// Note that single Shared Worker can be accessed from multiple tabs, so Lumina may
    /// already have been started. Otherwise it needs to be started with [`NodeDriver::start`].
    ///
    /// Requires serving a worker script and providing an url to it. The script should look like
    /// so (the import statement may vary depending on your js-bundler):
    /// ```js
    /// import init, { run_worker } from 'lumina_node_wasm.js';
    ///
    /// Error.stackTraceLimit = 99;
    ///
    /// // for SharedWorker we queue incoming connections
    /// // for dedicated Worker we queue incoming messages (coming from the single client)
    /// let queued = [];
    /// if (typeof SharedWorkerGlobalScope !== 'undefined' && self instanceof SharedWorkerGlobalScope) {
    ///   onconnect = (event) => {
    ///     queued.push(event)
    ///   }
    /// } else {
    ///   onmessage = (event) => {
    ///     queued.push(event);
    ///   }
    /// }
    ///
    /// init().then(() => {
    ///   console.log("starting worker, queued messages: ", queued.length);
    ///   run_worker(queued);
    /// })
    /// ```
    #[wasm_bindgen(constructor)]
    pub async fn new(
        worker_script_url: &str,
        worker_type: Option<NodeWorkerKind>,
    ) -> Result<NodeDriver> {
        // For chrome we default to running in a dedicated Worker because:
        // 1. Chrome Android does not support SharedWorkers at all
        // 2. On desktop Chrome, restarting Lumina's worker causes all network connections to fail.
        let default_worker_type = if is_chrome().unwrap_or(false) {
            NodeWorkerKind::Dedicated
        } else {
            NodeWorkerKind::Shared
        };

        let worker = AnyWorker::new(
            worker_type.unwrap_or(default_worker_type),
            worker_script_url,
            LUMINA_WORKER_NAME,
        )?;

        Ok(Self {
            client: WorkerClient::new(worker),
        })
    }

    /// Check whether Lumina is currently running
    pub async fn is_running(&self) -> Result<bool> {
        let command = NodeCommand::IsRunning;
        let response = self.client.exec(command).await?;
        let running = response.into_is_running().check_variant()?;

        Ok(running)
    }

    /// Start a node with the provided config, if it's not running
    pub async fn start(&self, config: WasmNodeConfig) -> Result<()> {
        let command = NodeCommand::StartNode(config);
        let response = self.client.exec(command).await?;
        let _ = response.into_node_started().check_variant()?;

        Ok(())
    }

    /// Get node's local peer ID.
    pub async fn local_peer_id(&self) -> Result<String> {
        let command = NodeCommand::GetLocalPeerId;
        let response = self.client.exec(command).await?;
        let peer_id = response.into_local_peer_id().check_variant()?;

        Ok(peer_id)
    }

    /// Get current [`PeerTracker`] info.
    pub async fn peer_tracker_info(&self) -> Result<JsValue> {
        let command = NodeCommand::GetPeerTrackerInfo;
        let response = self.client.exec(command).await?;
        let peer_info = response.into_peer_tracker_info().check_variant()?;

        Ok(to_value(&peer_info)?)
    }

    /// Wait until the node is connected to at least 1 peer.
    pub async fn wait_connected(&self) -> Result<()> {
        let command = NodeCommand::WaitConnected { trusted: false };
        let response = self.client.exec(command).await?;
        let _ = response.into_connected().check_variant()?;

        Ok(())
    }

    /// Wait until the node is connected to at least 1 trusted peer.
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        let command = NodeCommand::WaitConnected { trusted: true };
        let response = self.client.exec(command).await?;
        response.into_connected().check_variant()?
    }

    /// Get current network info.
    pub async fn network_info(&self) -> Result<NetworkInfoSnapshot> {
        let command = NodeCommand::GetNetworkInfo;
        let response = self.client.exec(command).await?;

        response.into_network_info().check_variant()?
    }

    /// Get all the multiaddresses on which the node listens.
    pub async fn listeners(&self) -> Result<Array> {
        let command = NodeCommand::GetListeners;
        let response = self.client.exec(command).await?;
        let listeners = response.into_listeners().check_variant()?;
        let result = listeners?.iter().map(js_value_from_display).collect();

        Ok(result)
    }

    /// Get all the peers that node is connected to.
    pub async fn connected_peers(&self) -> Result<Array> {
        let command = NodeCommand::GetConnectedPeers;
        let response = self.client.exec(command).await?;
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
        let response = self.client.exec(command).await?;
        response.into_set_peer_trust().check_variant()?
    }

    /// Request the head header from the network.
    pub async fn request_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::Head);
        let response = self.client.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request a header for the block with a given hash from the network.
    pub async fn request_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.client.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request a header for the block with a given height from the network.
    pub async fn request_header_by_height(&self, height: u64) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::ByHeight(height));
        let response = self.client.exec(command).await?;
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
        let response = self.client.exec(command).await?;
        let headers = response.into_headers().check_variant()?;

        headers.into()
    }

    /// Get current header syncing info.
    pub async fn syncer_info(&self) -> Result<JsValue> {
        let command = NodeCommand::GetSyncerInfo;
        let response = self.client.exec(command).await?;
        let syncer_info = response.into_syncer_info().check_variant()?;

        Ok(to_value(&syncer_info?)?)
    }

    /// Get the latest header announced in the network.
    pub async fn get_network_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::LastSeenNetworkHead;
        let response = self.client.exec(command).await?;
        let header = response.into_last_seen_network_head().check_variant()?;

        Ok(header)
    }

    /// Get the latest locally synced header.
    pub async fn get_local_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::Head);
        let response = self.client.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Get a synced header for the block with a given hash.
    pub async fn get_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.client.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Get a synced header for the block with a given height.
    pub async fn get_header_by_height(&self, height: u64) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::ByHeight(height));
        let response = self.client.exec(command).await?;
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
        let response = self.client.exec(command).await?;
        let headers = response.into_headers().check_variant()?;

        headers.into()
    }

    /// Get data sampling metadata of an already sampled height.
    pub async fn get_sampling_metadata(&self, height: u64) -> Result<JsValue> {
        let command = NodeCommand::GetSamplingMetadata { height };
        let response = self.client.exec(command).await?;
        let metadata = response.into_sampling_metadata().check_variant()?;

        Ok(to_value(&metadata?)?)
    }

    /// Requests SharedWorker running lumina to close. Any events received afterwards wont
    /// be processed and new NodeClient needs to be created to restart a node.
    pub async fn close(&self) -> Result<()> {
        let command = NodeCommand::CloseWorker;
        let response = self.client.exec(command).await?;
        response.into_worker_closed().check_variant()?;

        Ok(())
    }

    /// Returns a [`BroadcastChannel`] for events generated by [`Node`].
    pub async fn events_channel(&self) -> Result<BroadcastChannel> {
        let command = NodeCommand::GetEventsChannelName;
        let response = self.client.exec(command).await?;
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
                .filter(|addr| addr.iter().any(|proto| proto == Protocol::WebTransport))
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

        let p2p_bootnodes = self
            .bootnodes
            .iter()
            .map(|addr| addr.parse())
            .collect::<std::result::Result<_, _>>()
            .context("bootstrap multiaddr invalid")?;

        Ok(NodeConfig {
            network_id: network_id.to_string(),
            p2p_bootnodes,
            p2p_local_keypair,
            p2p_listen_on: vec![],
            blockstore,
            store,
        })
    }
}
