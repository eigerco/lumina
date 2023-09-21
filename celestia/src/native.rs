use std::env;

use anyhow::{Context, Result};
use celestia_node::node::{Node, NodeConfig};
use celestia_node::p2p::P2pService;
use celestia_node::store::InMemoryStore;
use celestia_rpc::prelude::*;
use libp2p::{core::upgrade::Version, identity, noise, tcp, yamux, Multiaddr, Transport};
use tracing::info;

const WS_URL: &str = "ws://localhost:26658";

pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();
    let _guard = init_tracing();

    let store = InMemoryStore::new();

    let bridge_ma = fetch_bridge_multiaddr().await?;
    let p2p_local_keypair = identity::Keypair::generate_ed25519();

    let p2p_transport = tcp::tokio::Transport::default()
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(&p2p_local_keypair)?)
        .multiplex(yamux::Config::default())
        .boxed();

    let node = Node::new(NodeConfig {
        network_id: "private".to_string(),
        p2p_transport,
        p2p_local_keypair,
        p2p_bootstrap_peers: vec![bridge_ma],
        p2p_listen_on: vec![],
        store,
    })
    .await
    .unwrap();

    node.p2p().wait_connected().await?;

    let header = node.p2p().get_header_by_height(1).await?;
    info!("{header:?}");

    Ok(())
}

/// Get the address of the local bridge node
async fn fetch_bridge_multiaddr() -> Result<Multiaddr> {
    let auth_token = env::var("CELESTIA_NODE_AUTH_TOKEN_ADMIN")?;
    let client = celestia_rpc::client::new_websocket(WS_URL, Some(&auth_token)).await?;
    let bridge_info = client.p2p_info().await?;
    info!("bridge id: {:?}", bridge_info.id);
    info!("bridge listens on: {:?}", bridge_info.addrs);

    let bridge_ma = bridge_info
        .addrs
        .into_iter()
        .find(|ma| ma.protocol_stack().any(|protocol| protocol == "tcp"))
        .context("Bridge doesn't listen on tcp")?;

    Ok(bridge_ma)
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
