use std::env;
use std::time::Duration;

use anyhow::{bail, Result};
use celestia_node::node::{Node, NodeConfig};
use celestia_node::p2p::P2pService;
use celestia_node::store::InMemoryStore;
use celestia_rpc::prelude::*;
use clap::{Parser, ValueEnum};
use libp2p::{
    core::upgrade::Version, dns::TokioDnsConfig, identity, multiaddr::Protocol, noise, tcp, yamux,
    Multiaddr, Transport,
};
use tokio::time::sleep;
use tracing::info;

#[derive(Debug, Parser)]
struct Args {
    /// Network to connect
    #[arg(short, long, value_enum, default_value_t)]
    network: Network,

    /// Listening addresses
    #[arg(short, long = "listen")]
    listen_addrs: Vec<Multiaddr>,

    /// Bootnode
    #[arg(short, long = "bootnode")]
    bootnodes: Vec<Multiaddr>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Network {
    Arabica,
    Mocha,
    #[default]
    Private,
}

pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();
    let args = Args::parse();
    let _guard = init_tracing();

    let store = InMemoryStore::new();

    let p2p_local_keypair = identity::Keypair::generate_ed25519();

    let p2p_bootstrap_peers = if args.bootnodes.is_empty() {
        network_bootnodes(args.network).await?
    } else {
        args.bootnodes
    };

    let network_id = network_id(args.network).to_owned();

    let p2p_transport = TokioDnsConfig::system(tcp::tokio::Transport::new(tcp::Config::default()))?
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(&p2p_local_keypair)?)
        .multiplex(yamux::Config::default())
        .boxed();

    let node = Node::new(NodeConfig {
        network_id,
        p2p_transport,
        p2p_local_keypair,
        p2p_bootstrap_peers,
        p2p_listen_on: args.listen_addrs,
        store,
    })
    .await
    .unwrap();

    node.p2p().wait_connected().await?;

    // We have nothing else to do, but we want to keep main alive
    loop {
        sleep(Duration::from_secs(1)).await;
    }
}

fn network_id(network: Network) -> &'static str {
    match network {
        Network::Arabica => "arabica-10",
        Network::Mocha => "mocha-4",
        Network::Private => "private",
    }
}

async fn network_bootnodes(network: Network) -> Result<Vec<Multiaddr>> {
    match network {
        Network::Arabica => Ok(
            [
                "/dns4/da-bridge.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWM3e9MWtyc8GkP8QRt74Riu17QuhGfZMytB2vq5NwkWAu",
                "/dns4/da-bridge-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWKj8mcdiBGxQRe1jqhaMnh2tGoC3rPDmr5UH2q8H4WA9M",
                "/dns4/da-full-1.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWBWkgmN7kmJSFovVrCjkeG47FkLGq7yEwJ2kEqNKCsBYk",
                "/dns4/da-full-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWRByRF67a2kVM2j4MP5Po3jgTw7H2iL2Spu8aUwPkrRfP",
            ]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect()
        ),
        Network::Mocha => Ok(
            [
                "/dns4/da-bridge-mocha-4.celestia-mocha.com/tcp/2121/p2p/12D3KooWCBAbQbJSpCpCGKzqz3rAN4ixYbc63K68zJg9aisuAajg",
                "/dns4/da-bridge-mocha-4-2.celestia-mocha.com/tcp/2121/p2p/12D3KooWK6wJkScGQniymdWtBwBuU36n6BRXp9rCDDUD6P5gJr3G",
                "/dns4/da-full-1-mocha-4.celestia-mocha.com/tcp/2121/p2p/12D3KooWCUHPLqQXZzpTx1x3TAsdn3vYmTNDhzg66yG8hqoxGGN8",
                "/dns4/da-full-2-mocha-4.celestia-mocha.com/tcp/2121/p2p/12D3KooWR6SHsXPkkvhCRn6vp1RqSefgaT1X1nMNvrVjU2o3GoYy",
            ]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect()
        ),
        Network::Private => Ok(
            fetch_bridge_multiaddrs("ws://localhost:26658").await?
        )
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
