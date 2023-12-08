use std::str::FromStr;

use celestia_types::hash::Hash;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Network {
    #[default]
    Mainnet,
    Arabica,
    Mocha,
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
        Network::Mainnet => "celestia",
    }
}

pub fn network_genesis(network: Network) -> Option<Hash> {
    let hex = match network {
        Network::Mainnet => "6BE39EFD10BA412A9DB5288488303F5DD32CF386707A5BEF33617F4C43301872",
        Network::Arabica => "5904E55478BA4B3002EE885621E007A2A6A2399662841912219AECD5D5CBE393",
        Network::Mocha => "B93BBE20A0FBFDF955811B6420F8433904664D45DB4BF51022BE4200C1A1680D",
        Network::Private => return None,
    };

    let bytes = hex::decode(hex).expect("failed decoding genesis hash");
    let array = bytes.try_into().expect("invalid genesis hash lenght");

    Some(Hash::Sha256(array))
}

pub fn canonical_network_bootnodes(network: Network) -> impl Iterator<Item = Multiaddr> {
    let peers: &[_] = match network {
        Network::Mainnet => &[
            "/dns4/lumina.eiger.co/tcp/2121/p2p/12D3KooW9z4jLqwodwNRcSa5qgcSgtJ13kN7CYLcwZQjPRYodqWx",
            "/dns4/lumina.eiger.co/udp/2121/quic-v1/webtransport/p2p/12D3KooW9z4jLqwodwNRcSa5qgcSgtJ13kN7CYLcwZQjPRYodqWx",
            "/dns4/da-bridge-1.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWSqZaLcn5Guypo2mrHr297YPJnV8KMEMXNjs3qAS8msw8",
            "/dns4/da-bridge-2.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWQpuTFELgsUypqp9N4a1rKBccmrmQVY8Em9yhqppTJcXf",
            "/dns4/da-bridge-3.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWSGa4huD6ts816navn7KFYiStBiy5LrBQH1HuEahk4TzQ",
            "/dns4/da-bridge-4.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWHBXCmXaUNat6ooynXG837JXPsZpSTeSzZx6DpgNatMmR",
            "/dns4/da-bridge-5.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWDGTBK1a2Ru1qmnnRwP6Dmc44Zpsxi3xbgFk7ATEPfmEU",
            "/dns4/da-bridge-6.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWLTUFyf3QEGqYkHWQS2yCtuUcL78vnKBdXU5gABM1YDeH",
            "/dns4/da-full-1.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWKZCMcwGCYbL18iuw3YVpAZoyb1VBGbx9Kapsjw3soZgr",
            "/dns4/da-full-2.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWE3fmRtHgfk9DCuQFfY3H3JYEnTU3xZozv1Xmo8KWrWbK",
            "/dns4/da-full-3.celestia-bootstrap.net/tcp/2121/p2p/12D3KooWK6Ftsd4XsWCsQZgZPNhTrE5urwmkoo5P61tGvnKmNVyv",
        ],
        Network::Arabica => &[
            "/dns4/da-bridge.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWM3e9MWtyc8GkP8QRt74Riu17QuhGfZMytB2vq5NwkWAu",
            "/dns4/da-bridge-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWKj8mcdiBGxQRe1jqhaMnh2tGoC3rPDmr5UH2q8H4WA9M",
            "/dns4/da-full-1.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWBWkgmN7kmJSFovVrCjkeG47FkLGq7yEwJ2kEqNKCsBYk",
            "/dns4/da-full-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWRByRF67a2kVM2j4MP5Po3jgTw7H2iL2Spu8aUwPkrRfP",
        ],
        Network::Mocha => &[
            "/dns4/da-bridge-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWCBAbQbJSpCpCGKzqz3rAN4ixYbc63K68zJg9aisuAajg",
            "/dns4/da-bridge-mocha-4-2.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWK6wJkScGQniymdWtBwBuU36n6BRXp9rCDDUD6P5gJr3G",
            "/dns4/da-full-1-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWCUHPLqQXZzpTx1x3TAsdn3vYmTNDhzg66yG8hqoxGGN8",
            "/dns4/da-full-2-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWR6SHsXPkkvhCRn6vp1RqSefgaT1X1nMNvrVjU2o3GoYy",
        ],
        Network::Private => &[],
    };
    peers
        .iter()
        .map(|s| s.parse().expect("Invalid bootstrap address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_genesis() {
        let mainnet = network_genesis(Network::Mainnet);
        assert!(mainnet.is_some());

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
        let mainnet = canonical_network_bootnodes(Network::Mainnet);
        assert_ne!(mainnet.count(), 0);

        let arabica = canonical_network_bootnodes(Network::Arabica);
        assert_ne!(arabica.count(), 0);

        let mocha = canonical_network_bootnodes(Network::Mocha);
        assert_ne!(mocha.count(), 0);

        let private = canonical_network_bootnodes(Network::Private);
        assert_eq!(private.count(), 0);
    }
}
