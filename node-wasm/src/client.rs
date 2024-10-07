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
use crate::ports::WorkerClient;
use crate::utils::{
    is_safari, js_value_from_display, request_storage_persistence, resolve_dnsaddr_multiaddress,
    Network,
};
use crate::wrapper::libp2p::NetworkInfoSnapshot;
use crate::wrapper::node::{PeerTrackerInfoSnapshot, SyncingInfoSnapshot};

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

/// `NodeClient` is responsible for steering [`NodeWorker`] by sending it commands and receiving
/// responses over the provided port.
///
/// [`NodeWorker`]: crate::worker::NodeWorker
#[wasm_bindgen]
struct NodeClient {
    worker: WorkerClient,
}

#[wasm_bindgen]
impl NodeClient {
    /// Create a new connection to a Lumina node running in [`NodeWorker`]. Provided `port` is
    /// expected to have `MessagePort`-like interface for sending and receiving messages.
    #[wasm_bindgen(constructor)]
    pub async fn new(port: JsValue) -> Result<NodeClient> {
        // Safari doesn't have the `navigator.storage()` api
        if !is_safari()? {
            if let Err(e) = request_storage_persistence().await {
                error!("Error requesting storage persistence: {e}");
            }
        }

        Ok(Self {
            worker: WorkerClient::new(port)?,
        })
    }

    /// Establish a new connection to the existing worker over provided port
    #[wasm_bindgen(js_name = addConnectionToWorker)]
    pub async fn add_connection_to_worker(&self, port: &JsValue) -> Result<()> {
        self.worker.add_connection_to_worker(port).await
    }

    /// Check whether Lumina is currently running
    #[wasm_bindgen(js_name = isRunning)]
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
    #[wasm_bindgen(js_name = localPeerId)]
    pub async fn local_peer_id(&self) -> Result<String> {
        let command = NodeCommand::GetLocalPeerId;
        let response = self.worker.exec(command).await?;
        let peer_id = response.into_local_peer_id().check_variant()?;

        Ok(peer_id)
    }

    /// Get current [`PeerTracker`] info.
    #[wasm_bindgen(js_name = peerTrackerInfo)]
    pub async fn peer_tracker_info(&self) -> Result<PeerTrackerInfoSnapshot> {
        let command = NodeCommand::GetPeerTrackerInfo;
        let response = self.worker.exec(command).await?;
        let peer_info = response.into_peer_tracker_info().check_variant()?;

        Ok(peer_info.into())
    }

    /// Wait until the node is connected to at least 1 peer.
    #[wasm_bindgen(js_name = waitConnected)]
    pub async fn wait_connected(&self) -> Result<()> {
        let command = NodeCommand::WaitConnected { trusted: false };
        let response = self.worker.exec(command).await?;
        let _ = response.into_connected().check_variant()?;

        Ok(())
    }

    /// Wait until the node is connected to at least 1 trusted peer.
    #[wasm_bindgen(js_name = waitConnectedTrusted)]
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        let command = NodeCommand::WaitConnected { trusted: true };
        let response = self.worker.exec(command).await?;
        response.into_connected().check_variant()?
    }

    /// Get current network info.
    #[wasm_bindgen(js_name = networkInfo)]
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
    #[wasm_bindgen(js_name = connectedPeers)]
    pub async fn connected_peers(&self) -> Result<Array> {
        let command = NodeCommand::GetConnectedPeers;
        let response = self.worker.exec(command).await?;
        let peers = response.into_connected_peers().check_variant()?;
        let result = peers?.iter().map(js_value_from_display).collect();

        Ok(result)
    }

    /// Trust or untrust the peer with a given ID.
    #[wasm_bindgen(js_name = setPeerTrust)]
    pub async fn set_peer_trust(&self, peer_id: &str, is_trusted: bool) -> Result<()> {
        let command = NodeCommand::SetPeerTrust {
            peer_id: peer_id.parse()?,
            is_trusted,
        };
        let response = self.worker.exec(command).await?;
        response.into_set_peer_trust().check_variant()?
    }

    /// Request the head header from the network.
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = requestHeadHeader)]
    pub async fn request_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::Head);
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request a header for the block with a given hash from the network.
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = requestHeaderByHash)]
    pub async fn request_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request a header for the block with a given height from the network.
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = requestHeaderByHeight)]
    pub async fn request_header_by_height(&self, height: u64) -> Result<JsValue> {
        let command = NodeCommand::RequestHeader(SingleHeaderQuery::ByHeight(height));
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Request headers in range (from, from + amount] from the network.
    ///
    /// The headers will be verified with the `from` header.
    ///
    /// Returns an array of javascript objects with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = requestVerifiedHeaders)]
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
    #[wasm_bindgen(js_name = syncerInfo)]
    pub async fn syncer_info(&self) -> Result<SyncingInfoSnapshot> {
        let command = NodeCommand::GetSyncerInfo;
        let response = self.worker.exec(command).await?;
        let syncer_info = response.into_syncer_info().check_variant()?;

        Ok(syncer_info?.into())
    }

    /// Get the latest header announced in the network.
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = getNetworkHeadHeader)]
    pub async fn get_network_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::LastSeenNetworkHead;
        let response = self.worker.exec(command).await?;
        let header = response.into_last_seen_network_head().check_variant()?;

        header.into()
    }

    /// Get the latest locally synced header.
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = getLocalHeadHeader)]
    pub async fn get_local_head_header(&self) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::Head);
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Get a synced header for the block with a given hash.
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = getHeaderByHash)]
    pub async fn get_header_by_hash(&self, hash: &str) -> Result<JsValue> {
        let command = NodeCommand::GetHeader(SingleHeaderQuery::ByHash(hash.parse()?));
        let response = self.worker.exec(command).await?;
        let header = response.into_header().check_variant()?;

        header.into()
    }

    /// Get a synced header for the block with a given height.
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = getHeaderByHeight)]
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
    ///
    /// Returns an array of javascript objects with given structure:
    /// https://docs.rs/celestia-types/latest/celestia_types/struct.ExtendedHeader.html
    #[wasm_bindgen(js_name = getHeaders)]
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
    ///
    /// Returns a javascript object with given structure:
    /// https://docs.rs/lumina-node/latest/lumina_node/store/struct.SamplingMetadata.html
    #[wasm_bindgen(js_name = getSamplingMetadata)]
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
    #[wasm_bindgen(js_name = eventsChannel)]
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::worker::NodeWorker;
    use celestia_rpc::{prelude::*, Client};
    use celestia_types::ExtendedHeader;
    use libp2p::{multiaddr::Protocol, Multiaddr};
    use rexie::Rexie;
    use serde_wasm_bindgen::from_value;
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::{
        fmt::time::UtcTime, layer::SubscriberExt, util::SubscriberInitExt, Layer,
    };
    use tracing_web::MakeConsoleWriter;
    use wasm_bindgen_futures::spawn_local;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::MessageChannel;

    const WS_URL: &str = "ws://localhost:36658";

    pub async fn fetch_bridge_info(client: &Client) -> Multiaddr {
        let bridge_info = client.p2p_info().await.unwrap();

        let mut ma = bridge_info
            .addrs
            .into_iter()
            .find(|ma| {
                let not_localhost = !ma
                    .iter()
                    .any(|prot| prot == Protocol::Ip4("127.0.0.1".parse().unwrap()));
                let webtransport = ma
                    .protocol_stack()
                    .any(|protocol| protocol == "webtransport");
                not_localhost && webtransport
            })
            .expect("Bridge doesn't listen on webtransport");

        if !ma.protocol_stack().any(|protocol| protocol == "p2p") {
            ma.push(Protocol::P2p(bridge_info.id.into()))
        }

        tracing::info!("{ma:?}");

        ma
    }

    async fn remove_database() -> rexie::Result<()> {
        Rexie::delete("private").await?;
        Rexie::delete("private-blockstore").await?;
        Ok(())
    }

    fn setup_logs() {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_timer(UtcTime::rfc_3339()) // std::time is not available in browsers
            .with_writer(MakeConsoleWriter) // write events to the console
            .with_filter(LevelFilter::DEBUG); // TODO: allow customizing the log level
        tracing_subscriber::registry().with(fmt_layer).init();
    }

    #[wasm_bindgen_test]
    async fn request_network_head_header() {
        remove_database().await.expect("failed to clear db");
        setup_logs();
        let rpc_client = Client::new(WS_URL).await.unwrap();

        let message_channel = MessageChannel::new().unwrap();
        let mut worker = NodeWorker::new(message_channel.port1().into());
        let client = NodeClient::new(message_channel.port2().into())
            .await
            .unwrap();

        let ma = fetch_bridge_info(&rpc_client).await;

        spawn_local(async move {
            worker.run().await.unwrap();
        });

        assert!(!client.is_running().await.unwrap());

        client
            .start(WasmNodeConfig {
                network: Network::Private,
                bootnodes: vec![format!("{ma}")],
            })
            .await
            .unwrap();

        assert!(client.is_running().await.unwrap());

        client.wait_connected_trusted().await.unwrap();

        let info = client.network_info().await.unwrap();
        assert_eq!(info.num_peers, 1);

        let bridge_head_header = rpc_client.header_network_head().await.unwrap();
        let head_header: ExtendedHeader =
            from_value(client.request_head_header().await.unwrap()).unwrap();
        assert_eq!(head_header, bridge_head_header)
    }

    #[wasm_bindgen_test]
    async fn discover_network_peers() {
        remove_database().await.expect("failed to clear db");
        setup_logs();
        let rpc_client = Client::new(WS_URL).await.unwrap();

        let message_channel = MessageChannel::new().unwrap();
        let mut worker = NodeWorker::new(message_channel.port1().into());
        let client = NodeClient::new(message_channel.port2().into())
            .await
            .unwrap();

        let ma = fetch_bridge_info(&rpc_client).await;

        spawn_local(async move {
            worker.run().await.unwrap();
        });

        assert!(!client.is_running().await.unwrap());

        client
            .start(WasmNodeConfig {
                network: Network::Private,
                bootnodes: vec![format!("{ma}")],
            })
            .await
            .unwrap();

        client.wait_connected_trusted().await.unwrap();
        let info = client.network_info().await.unwrap();
        assert_eq!(info.num_peers, 1);

        gloo_timers::future::sleep(Duration::from_secs(10)).await;

        client.wait_connected_trusted().await.unwrap();
        let info = client.network_info().await.unwrap();
        assert_eq!(info.num_peers, 2);

    }
}
