use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use celestia_rpc::prelude::*;
use celestia_rpc::Client;
use clap::Parser;
use directories::ProjectDirs;
use libp2p::{identity, multiaddr::Protocol, Multiaddr};
use lumina_node::blockstore::RedbBlockstore;
use lumina_node::events::NodeEvent;
use lumina_node::network::{canonical_network_bootnodes, network_id, Network};
use lumina_node::node::{Node, NodeConfig};
use lumina_node::store::{RedbStore, Store};
use tokio::task::spawn_blocking;
use tracing::info;
use tracing::warn;

use crate::common::ArgNetwork;

const CELESTIA_LOCAL_BRIDGE_RPC_ADDR: &str = "ws://localhost:26658";

#[derive(Debug, Parser)]
pub(crate) struct Params {
    /// Network to connect.
    #[arg(short, long, value_enum, default_value_t)]
    pub(crate) network: ArgNetwork,

    /// Listening addresses. Can be used multiple times.
    #[arg(short, long = "listen")]
    pub(crate) listen_addrs: Vec<Multiaddr>,

    /// Bootnode multiaddr, including peer id. Can be used multiple times.
    #[arg(short, long = "bootnode")]
    pub(crate) bootnodes: Vec<Multiaddr>,

    /// Persistent header store path.
    #[arg(short, long = "store")]
    pub(crate) store: Option<PathBuf>,
}

pub(crate) async fn run(args: Params) -> Result<()> {
    let network = args.network.into();
    let p2p_local_keypair = identity::Keypair::generate_ed25519();

    let p2p_bootnodes = if args.bootnodes.is_empty() {
        match network {
            Network::Private => fetch_bridge_multiaddrs(CELESTIA_LOCAL_BRIDGE_RPC_ADDR).await?,
            network => canonical_network_bootnodes(network).collect(),
        }
    } else {
        args.bootnodes
    };

    {
        let network_id = network_id(network).to_owned();
        info!("Initializing store");
        let db = open_db(args.store.clone(), &network_id).await?;
        let store = RedbStore::new(db.clone()).await?;
        let blockstore = RedbBlockstore::new(db);

        let stored_ranges = store.get_stored_header_ranges().await?;
        if stored_ranges.is_empty() {
            info!("Initialised new store");
        } else {
            info!("Initialised store, present headers: {stored_ranges}");
        }

        let (_node, mut events) = Node::new_subscribed(NodeConfig {
            network_id,
            p2p_local_keypair: p2p_local_keypair.clone(),
            p2p_bootnodes: p2p_bootnodes.clone(),
            p2p_listen_on: args.listen_addrs.clone(),
            sync_batch_size: 512,
            blockstore,
            store,
        })
        .await
        .context("Failed to start node")?;

        let now = std::time::Instant::now();

        while let Ok(ev) = events.recv().await {
            if now.elapsed() >= std::time::Duration::from_secs(30) {
                break;
            }

            match ev.event {
                // Skip noisy events
                NodeEvent::ShareSamplingResult { .. } => continue,
                event if event.is_error() => warn!("{event}"),
                event => info!("{event}"),
            }
        }

        _node.stop().await;
    }

    warn!("RESTARTING");
    info!("Initializing store");
    let network_id = network_id(network).to_owned();
    let db = open_db(args.store, &network_id).await?;
    let store = RedbStore::new(db.clone()).await?;
    let blockstore = RedbBlockstore::new(db);

    let stored_ranges = store.get_stored_header_ranges().await?;
    if stored_ranges.is_empty() {
        info!("Initialised new store");
    } else {
        info!("Initialised store, present headers: {stored_ranges}");
    }

    let (_node, mut events) = Node::new_subscribed(NodeConfig {
        network_id,
        p2p_local_keypair,
        p2p_bootnodes,
        p2p_listen_on: args.listen_addrs,
        sync_batch_size: 512,
        blockstore,
        store,
    })
    .await
    .context("Failed to start node")?;

    while let Ok(ev) = events.recv().await {
        match ev.event {
            // Skip noisy events
            NodeEvent::ShareSamplingResult { .. } => continue,
            event if event.is_error() => warn!("{event}"),
            event => info!("{event}"),
        }
    }

    Ok(())
}

async fn open_db(path: Option<PathBuf>, network_id: &str) -> Result<Arc<redb::Database>> {
    let network_id = network_id.to_owned();

    spawn_blocking(move || {
        use std::fs;

        if let Some(path) = path {
            let db = redb::Database::create(path)?;
            return Ok(Arc::new(db));
        }

        let cache_dir = ProjectDirs::from("co", "eiger", "lumina")
            .context("failed to construct project path")?
            .cache_dir()
            .join(&network_id)
            .to_owned();

        let old_cache_dir = ProjectDirs::from("co", "eiger", "celestia")
            .context("failed to construct project path")?
            .cache_dir()
            .join(&network_id)
            .to_owned();

        if is_sled_db(&old_cache_dir) {
            warn!("Removing deprecated store {}", old_cache_dir.display());
            fs::remove_dir_all(&old_cache_dir)?;
        }

        if is_sled_db(&cache_dir) {
            warn!("Removing deprecated store {}", cache_dir.display());
            fs::remove_dir_all(&cache_dir)?;
        }

        // Directories need to pre-exist
        fs::create_dir_all(&cache_dir)?;

        let path = cache_dir.join("db");
        let db = redb::Database::create(path)?;

        Ok(Arc::new(db))
    })
    .await?
}

fn is_sled_db(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    path.join("blobs").is_dir() && path.join("conf").is_file() && path.join("db").is_file()
}

/// Get the address of the local bridge node
async fn fetch_bridge_multiaddrs(ws_url: &str) -> Result<Vec<Multiaddr>> {
    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")
        .context("Missing CELESTIA_NODE_AUTH_TOKEN_ADMIN environment variable")?;
    let client = Client::new(ws_url, Some(&auth_token)).await?;
    let bridge_info = client.p2p_info().await?;

    info!("bridge id: {:?}", bridge_info.id);
    info!("bridge listens on: {:?}", bridge_info.addrs);

    let addrs = bridge_info
        .addrs
        .into_iter()
        .filter(|ma| ma.protocol_stack().any(|protocol| protocol == "tcp"))
        .map(|mut ma| {
            if !ma.protocol_stack().any(|protocol| protocol == "p2p") {
                ma.push(Protocol::P2p(bridge_info.id.into()))
            }
            ma
        })
        .collect::<Vec<_>>();

    if addrs.is_empty() {
        bail!("Bridge doesn't listen on tcp");
    }

    Ok(addrs)
}
