use std::str::FromStr;

use celestia_types::hash::Hash;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Network {
    Arabica,
    Mocha,
    #[default]
    Private,
}

#[derive(Debug, Error)]
#[error("unknown network {0}")]
pub struct UnknownNetworkError(String);

impl FromStr for Network {
    type Err = UnknownNetworkError;

    fn from_str(network_id: &str) -> Result<Self, Self::Err> {
        match network_id {
            "arabica-10" => Ok(Network::Arabica),
            "mocha-4" => Ok(Network::Mocha),
            "private" => Ok(Network::Private),
            network => Err(UnknownNetworkError(network.to_string())),
        }
    }
}

pub fn network_id(network: Network) -> &'static str {
    match network {
        Network::Arabica => "arabica-10",
        Network::Mocha => "mocha-4",
        Network::Private => "private",
    }
}

pub fn network_genesis(network: Network) -> Option<Hash> {
    let hex = match network {
        Network::Arabica => "5904E55478BA4B3002EE885621E007A2A6A2399662841912219AECD5D5CBE393",
        Network::Mocha => "B93BBE20A0FBFDF955811B6420F8433904664D45DB4BF51022BE4200C1A1680D",
        Network::Private => return None,
    };

    let bytes = hex::decode(hex).expect("failed decoding genesis hash");
    let array = bytes.try_into().expect("invalid genesis hash lenght");

    Some(Hash::Sha256(array))
}

pub fn canonical_network_bootnodes(network: Network) -> Vec<Multiaddr> {
    match network {
        Network::Arabica => [
                "/dns4/da-bridge.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWM3e9MWtyc8GkP8QRt74Riu17QuhGfZMytB2vq5NwkWAu",
                "/dns4/da-bridge-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWKj8mcdiBGxQRe1jqhaMnh2tGoC3rPDmr5UH2q8H4WA9M",
                "/dns4/da-full-1.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWBWkgmN7kmJSFovVrCjkeG47FkLGq7yEwJ2kEqNKCsBYk",
                "/dns4/da-full-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWRByRF67a2kVM2j4MP5Po3jgTw7H2iL2Spu8aUwPkrRfP",
            ]
            .iter()
            .map(|s| s.parse())
            .collect(),
        Network::Mocha => [
                // 40.85.94.176 is a node set up for testing QUIC/WebTransport since official nodes
                // don't have that enabled currently
                "/ip4/40.85.94.176/tcp/2121/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
                "/ip4/40.85.94.176/udp/2121/quic-v1/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",
                "/ip4/40.85.94.176/udp/2121/quic-v1/webtransport/certhash/uEiCx1md1BATJ_0NXAjp3KOuwRYG1535E7kUzFdMq8aPaWw/certhash/uEiB99_E1dyPw-nj0S4OYN9Blv3U0MG9i6EI6nBJ-VanZ-A/p2p/12D3KooWNJ3Nf1DTQTz8JZogg2eSvPKKKv8itC6fxxspe4C6bizs",

                "/dns4/da-bridge-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWCBAbQbJSpCpCGKzqz3rAN4ixYbc63K68zJg9aisuAajg",
                "/dns4/da-bridge-mocha-4-2.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWK6wJkScGQniymdWtBwBuU36n6BRXp9rCDDUD6P5gJr3G",
                "/dns4/da-full-1-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWCUHPLqQXZzpTx1x3TAsdn3vYmTNDhzg66yG8hqoxGGN8",
                "/dns4/da-full-2-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWR6SHsXPkkvhCRn6vp1RqSefgaT1X1nMNvrVjU2o3GoYy",
            ]
            .iter()
            .map(|s| s.parse())
            .collect(),
        Network::Private => Ok(vec![])
    }.expect("invalid bootnode address")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_genesis() {
        let arabica = network_genesis(Network::Arabica);
        assert!(arabica.is_some());

        let mocha = network_genesis(Network::Mocha);
        assert!(mocha.is_some());

        let private = network_genesis(Network::Private);
        assert!(private.is_none());
    }

    #[test]
    fn test_canonical_network_bootnodes() {
        // canonical_network_bootnodes works on const data, test it doesn't panic and the data is there
        let arabica = canonical_network_bootnodes(Network::Arabica);
        assert_ne!(arabica.len(), 0);

        let mocha = canonical_network_bootnodes(Network::Mocha);
        assert_ne!(mocha.len(), 0);

        let private = canonical_network_bootnodes(Network::Private);
        assert_eq!(private.len(), 0);
    }
}
