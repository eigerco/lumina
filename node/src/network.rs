//! Primitives and constants related to the networks supported by Celestia nodes.

use std::str::FromStr;

use celestia_types::hash::Hash;
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Supported Celestia networks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Network {
    /// Celestia mainnet.
    #[default]
    Mainnet,
    /// Arabica testnet.
    Arabica,
    /// Mocha testnet.
    Mocha,
    /// Private local network.
    Private,
}

/// Unknown network provided.
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

/// Get the string id of the given network.
impl Network {
    pub fn id(&self) -> &'static str {
        match self {
            Network::Arabica => "arabica-10",
            Network::Mocha => "mocha-4",
            Network::Private => "private",
            Network::Mainnet => "celestia",
        }
    }

    /// Get the hash of a genesis block for the given network.
    pub fn genesis(&self) -> Option<Hash> {
        let hex = match self {
            Network::Mainnet => "6BE39EFD10BA412A9DB5288488303F5DD32CF386707A5BEF33617F4C43301872",
            Network::Arabica => "5904E55478BA4B3002EE885621E007A2A6A2399662841912219AECD5D5CBE393",
            Network::Mocha => "B93BBE20A0FBFDF955811B6420F8433904664D45DB4BF51022BE4200C1A1680D",
            Network::Private => return None,
        };

        let bytes = hex::decode(hex).expect("failed decoding genesis hash");
        let array = bytes.try_into().expect("invalid genesis hash lenght");

        Some(Hash::Sha256(array))
    }

    /// Get official Celestia and Lumina bootnodes for the given network.
    pub fn canonical_bootnodes(&self) -> impl Iterator<Item = Multiaddr> {
        let peers: &[_] = match self {
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
            "/dns4/da-bridge-1.celestia-arabica-11.com/tcp/2121/p2p/12D3KooWGqwzdEqM54Dce6LXzfFr97Bnhvm6rN7KM7MFwdomfm4S",
            "/dns4/da-bridge-2.celestia-arabica-11.com/tcp/2121/p2p/12D3KooWCMGM5eZWVfCN9ZLAViGfLUWAfXP5pCm78NFKb9jpBtua",
            "/dns4/da-bridge-3.celestia-arabica-11.com/tcp/2121/p2p/12D3KooWEWuqrjULANpukDFGVoHW3RoeUU53Ec9t9v5cwW3MkVdQ",
            "/dns4/da-bridge-4.celestia-arabica-11.com/tcp/2121/p2p/12D3KooWLT1ysSrD7XWSBjh7tU1HQanF5M64dHV6AuM6cYEJxMPk",
        ],
        Network::Mocha => &[
            "/dns4/da-bridge-mocha-4.celestia-mocha.com/tcp/2121/p2p/12D3KooWCBAbQbJSpCpCGKzqz3rAN4ixYbc63K68zJg9aisuAajg",
            "/dns4/da-bridge-mocha-4-2.celestia-mocha.com/tcp/2121/p2p/12D3KooWK6wJkScGQniymdWtBwBuU36n6BRXp9rCDDUD6P5gJr3G",
            "/dns4/da-full-1-mocha-4.celestia-mocha.com/tcp/2121/p2p/12D3KooWCUHPLqQXZzpTx1x3TAsdn3vYmTNDhzg66yG8hqoxGGN8",
            "/dns4/da-full-2-mocha-4.celestia-mocha.com/tcp/2121/p2p/12D3KooWR6SHsXPkkvhCRn6vp1RqSefgaT1X1nMNvrVjU2o3GoYy",
        ],
        Network::Private => &[],
    };
        peers
            .iter()
            .map(|s| s.parse().expect("Invalid bootstrap address"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_genesis() {
        let mainnet = Network::Mainnet.genesis();
        assert!(mainnet.is_some());

        let arabica = Network::Arabica.genesis();
        assert!(arabica.is_some());

        let mocha = Network::Mocha.genesis();
        assert!(mocha.is_some());

        let private = Network::Private.genesis();
        assert!(private.is_none());
    }

    #[test]
    fn test_canonical_network_bootnodes() {
        // canonical_network_bootnodes works on const data, test it doesn't panic and the data is there
        let mainnet = Network::Mainnet.canonical_bootnodes();
        assert_ne!(mainnet.count(), 0);

        let arabica = Network::Arabica.canonical_bootnodes();
        assert_ne!(arabica.count(), 0);

        let mocha = Network::Mocha.canonical_bootnodes();
        assert_ne!(mocha.count(), 0);

        let private = Network::Private.canonical_bootnodes();
        assert_eq!(private.count(), 0);
    }
}
