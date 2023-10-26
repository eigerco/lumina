use std::net::SocketAddr;

use crate::{native, server};
use anyhow::{bail, Result};
use celestia_node::network::Network;
use clap::{error::ErrorKind, ArgGroup, CommandFactory, Parser, ValueEnum};
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::path::PathBuf;

const SERVER_DEFAULT_BIND_ADDR: &str = "127.0.0.1:9876";

#[derive(Debug, Parser)]
#[clap(group(ArgGroup::new("native_xor_browser")
             .args(&["store", "browser"])))]
pub(crate) struct Args {
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

    /// Serve wasm node which can be accessed with web browser
    #[arg(long)]
    browser: bool,
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum ArgNetwork {
    Arabica,
    Mocha,
    #[default]
    Private,
}

pub async fn run_cli() -> Result<()> {
    let _ = dotenvy::dotenv();
    let mut args = Args::parse();
    let _guard = init_tracing();

    if args.browser {
        let listen_addr = match args.listen_addrs.len() {
            0 => SERVER_DEFAULT_BIND_ADDR.parse().unwrap(),
            1 => multiaddr_to_socketaddr(args.listen_addrs.pop().unwrap())?,
            _ => Args::command()
                .error(
                    ErrorKind::TooManyValues,
                    "expected single listenting address when serving wasm node",
                )
                .exit(),
        };
        server::run(args.network, args.bootnodes, listen_addr).await
    } else {
        native::run(args).await
    }
}

pub fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
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

fn multiaddr_to_socketaddr(mut addr: Multiaddr) -> Result<SocketAddr> {
    let mut port = None;
    while let Some(proto) = addr.pop() {
        match proto {
            Protocol::Ip4(ipv4) => match port {
                Some(port) => return Ok(SocketAddr::new(ipv4.into(), port)),
                None => bail!("port not specified"),
            },
            Protocol::Ip6(ipv6) => match port {
                Some(port) => return Ok(SocketAddr::new(ipv6.into(), port)),
                None => bail!("port not specified"),
            },
            Protocol::Tcp(portnum) => match port {
                Some(_) => bail!("multiple ports specified"),
                None => port = Some(portnum),
            },
            Protocol::P2p(_) => {}
            _ => bail!("p2p protocol not supported here"),
        }
    }
    bail!("no listen address specified")
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
