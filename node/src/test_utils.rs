use celestia_types::test_utils::ExtendedHeaderGenerator;

use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed, upgrade::Version},
    identity::{self, Keypair},
    noise, tcp, yamux, PeerId, Transport,
};

use crate::{node::NodeConfig, store::InMemoryStore};

pub fn gen_filled_store(amount: u64) -> (InMemoryStore, ExtendedHeaderGenerator) {
    let s = InMemoryStore::new();
    let mut gen = ExtendedHeaderGenerator::new();

    let headers = gen.next_many(amount);

    for header in headers {
        s.append_single_unchecked(header)
            .expect("inserting test data failed");
    }

    (s, gen)
}

fn tcp_transport(local_keypair: &Keypair) -> Boxed<(PeerId, StreamMuxerBox)> {
    tcp::tokio::Transport::default()
        .upgrade(Version::V1Lazy)
        .authenticate(noise::Config::new(local_keypair).unwrap())
        .multiplex(yamux::Config::default())
        .boxed()
}

// helpers to use with struct update syntax to avoid spelling out all the details
pub fn test_node_config() -> NodeConfig<InMemoryStore> {
    let node_keypair = identity::Keypair::generate_ed25519();
    NodeConfig {
        network_id: "private".to_string(),
        p2p_transport: tcp_transport(&node_keypair),
        p2p_local_keypair: node_keypair,
        p2p_bootstrap_peers: vec![],
        p2p_listen_on: vec![],
        store: InMemoryStore::new(),
    }
}

pub fn listening_test_node_config() -> NodeConfig<InMemoryStore> {
    NodeConfig {
        p2p_listen_on: vec!["/ip4/0.0.0.0/tcp/0".parse().unwrap()],
        ..test_node_config()
    }
}

pub fn test_node_config_with_keypair(keypair: Keypair) -> NodeConfig<InMemoryStore> {
    NodeConfig {
        p2p_transport: tcp_transport(&keypair),
        p2p_local_keypair: keypair,
        ..test_node_config()
    }
}
