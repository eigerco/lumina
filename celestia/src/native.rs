use std::env;

use anyhow::{Context, Result};
use celestia_node::node::{Node, NodeConfig};
use celestia_rpc::prelude::*;
use libp2p::{core::upgrade::Version, identity, noise, tcp, yamux, Transport};

const WS_URL: &str = "ws://localhost:26658";

#[tokio::main]
pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();

    // Get the address of the local bridge node
    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")?;
    let client = celestia_rpc::client::new_websocket(WS_URL, Some(&auth_token)).await?;
    let bridge_info = client.p2p_info().await?;
    println!("bridge id: {:?}", bridge_info.id);
    println!("bridge listens on: {:?}", bridge_info.addrs);

    let bridge_ma = bridge_info
        .addrs
        .into_iter()
        .find(|ma| ma.protocol_stack().any(|protocol| protocol == "tcp"))
        .context("Bridge doesn't listen on tcp")?;

    let p2p_local_keypair = identity::Keypair::generate_ed25519();

    let p2p_transport = tcp::tokio::Transport::default()
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(&p2p_local_keypair)?)
        .multiplex(yamux::Config::default())
        .boxed();

    let _node = Node::new(NodeConfig {
        network_id: "private".to_string(),
        p2p_transport,
        p2p_local_keypair,
        p2p_bootstrap_peers: vec![bridge_ma],
        p2p_listen_on: vec![],
    })
    .await
    .unwrap();

    Ok(())
}
