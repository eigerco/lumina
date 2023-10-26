use std::env;
use std::time::Duration;

use crate::common::Args;
use anyhow::{bail, Context, Result};
use celestia_node::network::{canonical_network_bootnodes, network_genesis, network_id, Network};
use celestia_node::node::{Node, NodeConfig};
use celestia_node::store::SledStore;
use celestia_rpc::prelude::*;
use libp2p::{identity, multiaddr::Protocol, Multiaddr};
use tokio::time::sleep;
use tracing::info;

pub(crate) async fn run(args: Args) -> Result<()> {
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
