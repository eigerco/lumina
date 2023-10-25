use std::env;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use celestia_node::network::{canonical_network_bootnodes, network_genesis, network_id, Network};
use celestia_node::node::{Node, NodeConfig};
use celestia_node::store::SledStore;
use celestia_rpc::prelude::*;
use clap::{Parser, ValueEnum};
use libp2p::{identity, multiaddr::Protocol, Multiaddr};
use tokio::time::sleep;
use tracing::info;

#[derive(Debug, Parser)]
struct Args {
    /// Network to connect.
    #[arg(short, long, value_enum, default_value_t)]
    network: ArgNetwork,

    /// Listening addresses. Can be used multiple times.
    #[arg(short, long = "listen")]
    listen_addrs: Vec<Multiaddr>,

    /// Bootnode multiaddr, including peer id. Can be used multiple times.
    #[arg(short, long = "bootnode")]
    bootnodes: Vec<Multiaddr>,

    /// Persistent header store path.
    #[arg(short, long = "store")]
    store: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ArgNetwork {
    Arabica,
    Mocha,
    #[default]
    Private,
}

pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();
    let args = Args::parse();
    let _guard = init_tracing();

    let p2p_local_keypair = identity::Keypair::generate_ed25519();

    let network = args.network.into();
    let p2p_bootnodes = if args.bootnodes.is_empty() {
        match network {
            Network::Private => fetch_bridge_multiaddrs("ws://localhost:26658").await?,
            network => canonical_network_bootnodes(network)?,
        }
    } else {
        args.bootnodes
    };

    let network_id = network_id(network).to_owned();
    let genesis_hash = network_genesis(network)?;

    let store = if let Some(db_path) = args.store {
        SledStore::new_in_path(db_path).await?
    } else {
        SledStore::new(network_id.clone()).await?
    };

    if let Ok(store_height) = store.head_height().await {
        info!("Initialised store with head height: {store_height}");
    }

    let node = Node::new(NodeConfig {
        network_id,
        genesis_hash,
        p2p_local_keypair,
        p2p_bootnodes,
        p2p_listen_on: args.listen_addrs,
        store,
    })
    .await
    .context("Failed to start node")?;

    node.p2p().wait_connected_trusted().await?;

    // We have nothing else to do, but we want to keep main alive
    loop {
        sleep(Duration::from_secs(1)).await;
    }
}

/// Get the address of the local bridge node
async fn fetch_bridge_multiaddrs(ws_url: &str) -> Result<Vec<Multiaddr>> {
    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")?;
    let client = celestia_rpc::client::new_websocket(ws_url, Some(&auth_token)).await?;
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

fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());

    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .init();

    guard
}

impl From<ArgNetwork> for Network {
    fn from(network: ArgNetwork) -> Network {
        match network {
            ArgNetwork::Arabica => Network::Arabica,
            ArgNetwork::Mocha => Network::Mocha,
            ArgNetwork::Private => Network::Private,
        }
    }
}
