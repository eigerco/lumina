//! Tiny p2p node without any defined behaviour celestia can connect to so that we can test RPC p2p
//! calls

use celestia_rpc::p2p;
use futures::StreamExt;
use libp2p::{
    core::upgrade::Version,
    identify,
    identity::{self, Keypair},
    noise,
    swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Transport,
};
use tokio::{
    task::JoinHandle,
    time::{sleep, Duration},
};

// how long to wait during startup for node to start listening on interfaces, before we return a
// list of addresses
const NODE_ADDRESS_ACQUIRE_DELAY_TIME: Duration = Duration::from_millis(100);

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour {
    identify: identify::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

impl Behaviour {
    fn new(local_key: Keypair) -> Self {
        let identify =
            identify::Behaviour::new(identify::Config::new("".to_owned(), local_key.public()));

        Self {
            identify,
            keep_alive: keep_alive::Behaviour,
        }
    }
}

pub async fn start_tiny_node() -> anyhow::Result<(p2p::AddrInfo, JoinHandle<()>)> {
    async fn get_addresses<T: NetworkBehaviour>(swarm: &mut libp2p::Swarm<T>) -> Vec<Multiaddr> {
        let mut addresses = vec![];
        let timeout = sleep(NODE_ADDRESS_ACQUIRE_DELAY_TIME);
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                swarm_ev = swarm.select_next_some() => {
                    match swarm_ev {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            addresses.push(address);
                        },
                        _ => (),
                    }
                },
                () = &mut timeout => {
                    break;
                }
            }
        }

        return addresses;
    }

    // Create identity
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    log::debug!("local peer id: {local_peer_id:?}");

    // Setup swarm
    let transport = tcp::tokio::Transport::default()
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(&local_key)?)
        .multiplex(yamux::Config::default())
        .boxed();

    let mut swarm =
        SwarmBuilder::with_tokio_executor(transport, Behaviour::new(local_key), local_peer_id)
            .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    let addrs = get_addresses(&mut swarm).await;

    let task = tokio::task::spawn(async move {
        loop {
            swarm.select_next_some().await;
        }
    });

    let addr = p2p::AddrInfo {
        id: p2p::PeerId(local_peer_id),
        addrs,
    };
    log::debug!("Listening addresses: {addr:?}");

    return Ok((addr, task));
}
