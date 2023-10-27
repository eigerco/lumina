use celestia_node::network::{self, Network};
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn setup_logging() {
    console_error_panic_hook::set_once();

    tracing_wasm::set_as_global_default();
}

#[wasm_bindgen]
pub fn canonical_network_bootnodes(network: Network) -> JsValue {
    let mut bootnodes = network::canonical_network_bootnodes(network);

    if network == Network::Mocha {
        // 40.85.94.176 is a node set up for testing QUIC/WebTransport since official nodes
        // don't have that enabled currently
        let webtransport_bootnode_addrs = [
            "/ip4/40.85.94.176/tcp/2121/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
            "/ip4/40.85.94.176/udp/2121/quic-v1/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
            "/ip4/40.85.94.176/udp/2121/quic-v1/webtransport/certhash/uEiBf-OX4HzFK9owOpjdCifsDIWRO0SoD3j3vGKlq0pAXKw/certhash/uEiCx1md1BATJ_0NXAjp3KOuwRYG1535E7kUzFdMq8aPaWw/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
            "/ip4/40.85.94.176/udp/2121/quic-v1/p2p/12D3KooWQUYAApYb4DJnhS1QmAwRr5HRvUeHJYocchCpwEhCtDGu",
            "/ip4/40.85.94.176/udp/2121/quic-v1/webtransport/certhash/uEiBr4-sr95BpqfA-ttpjiLdjbGABhTvX8oxrTXf3Ubfibw/certhash/uEiBSVgyze9xG1UbbNuTwyEUWLPq7l2N9pyeQSs3OtEhGRg/p2p/12D3KooWQUYAApYb4DJnhS1QmAwRr5HRvUeHJYocchCpwEhCtDGu",
        ].iter().map(|s| s.parse().unwrap_throw());

        bootnodes.extend(webtransport_bootnode_addrs);
    }

    to_value(&bootnodes).unwrap_throw()
}

#[wasm_bindgen]
pub fn network_genesis(network: Network) -> JsValue {
    to_value(&network::network_genesis(network)).unwrap_throw()
}
