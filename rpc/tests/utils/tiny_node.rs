//! Tiny p2p node without any defined behaviour celestia can connect to so that we can test RPC p2p
//! calls

use celestia_types::p2p;
use futures::StreamExt;
use libp2p::{
    core::upgrade::Version,
    identity, noise,
    swarm::{dummy, NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux, PeerId, Transport,
};
use tokio::{
    sync::mpsc,
    time::{sleep, Duration},
};
use tracing::{debug, warn};

// how long to wait during startup for node to start listening on interfaces, before we return a
// list of addresses
const NODE_ADDRESS_ACQUIRE_DELAY_TIME: Duration = Duration::from_millis(100);

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour {
    dummy: dummy::Behaviour,
}

impl Behaviour {
    fn new() -> Self {
        Self {
            dummy: dummy::Behaviour,
        }
    }
}

pub async fn start_tiny_node() -> anyhow::Result<p2p::AddrInfo> {
    // Create identity
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    debug!("local peer id: {local_peer_id:?}");

    // Setup swarm
    let transport = tcp::tokio::Transport::default()
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(&local_key)?)
        .multiplex(yamux::Config::default())
        .boxed();

    let mut swarm = SwarmBuilder::with_tokio_executor(transport, Behaviour::new(), local_peer_id)
        .idle_connection_timeout(Duration::from_secs(u64::MAX))
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    let (addr_tx, mut addr_rx) = mpsc::channel(32);

    tokio::task::spawn(async move {
        loop {
            if let Some(SwarmEvent::NewListenAddr { address, .. }) = swarm.next().await {
                debug!("{address:?}");

                if addr_tx.send(address).await.is_err() {
                    warn!("received new addr after set startup time, unittests might not have all the node addresses");
                }
            }
        }
    });

    // give node second to acquire addresses and then gather all the ones we received
    sleep(NODE_ADDRESS_ACQUIRE_DELAY_TIME).await;
    addr_rx.close();

    let mut addrs = vec![];
    while let Some(addr) = addr_rx.recv().await {
        addrs.push(addr);
    }

    let addr = p2p::AddrInfo {
        id: p2p::PeerId(local_peer_id),
        addrs,
    };

    debug!("Listening addresses: {addr:?}");

    Ok(addr)
}
