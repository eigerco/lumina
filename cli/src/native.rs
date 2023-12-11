use std::env;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use celestia_rpc::prelude::*;
use celestia_rpc::Client;
use clap::Args;
use libp2p::{identity, multiaddr::Protocol, Multiaddr};
use lumina_node::network::{canonical_network_bootnodes, network_genesis, network_id, Network};
use lumina_node::node::{Node, NodeConfig};
use lumina_node::store::SledStore;
use tokio::time::sleep;
use tracing::info;

use crate::common::ArgNetwork;

const CELESTIA_LOCAL_BRIDGE_RPC_ADDR: &str = "ws://localhost:26658";

#[derive(Debug, Args)]
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

    let network_id = network_id(network).to_owned();
    let genesis_hash = network_genesis(network);

    info!("Initializing store");

    let store = if let Some(db_path) = args.store {
        SledStore::new_in_path(db_path).await?
    } else {
        SledStore::new(network_id.clone()).await?
    };

    match store.head_height().await {
        Ok(height) => info!("Initialised store with head height: {height}"),
        Err(_) => info!("Initialised new store"),
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

    node.wait_connected_trusted().await?;

    // We have nothing else to do, but we want to keep main alive
    loop {
        sleep(Duration::from_secs(1)).await;
    }
}

/// Get the address of the local bridge node
async fn fetch_bridge_multiaddrs(ws_url: &str) -> Result<Vec<Multiaddr>> {
    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")?;
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
