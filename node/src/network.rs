//! Primitives and constants related to the networks supported by Celestia nodes.

use std::str::FromStr;

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
            "arabica-11" => Ok(Network::Arabica),
            "mocha-4" => Ok(Network::Mocha),
            "private" => Ok(Network::Private),
            network => Err(UnknownNetworkError(network.to_string())),
        }
    }
}

/// Get the string id of the given network.
pub fn network_id(network: Network) -> &'static str {
    match network {
        Network::Arabica => "arabica-11",
        Network::Mocha => "mocha-4",
        Network::Private => "private",
        Network::Mainnet => "celestia",
    }
}

/// Get official Celestia and Lumina bootnodes for the given network.
pub fn canonical_network_bootnodes(network: Network) -> impl Iterator<Item = Multiaddr> {
    let peers: &[_] = match network {
        Network::Mainnet => &[
            "/dnsaddr/da-bridge-1.celestia-bootstrap.net/p2p/12D3KooWSqZaLcn5Guypo2mrHr297YPJnV8KMEMXNjs3qAS8msw8",
            "/dnsaddr/da-bridge-2.celestia-bootstrap.net/p2p/12D3KooWQpuTFELgsUypqp9N4a1rKBccmrmQVY8Em9yhqppTJcXf",
            "/dnsaddr/da-bridge-3.celestia-bootstrap.net/p2p/12D3KooWSGa4huD6ts816navn7KFYiStBiy5LrBQH1HuEahk4TzQ",
            "/dnsaddr/da-bridge-4.celestia-bootstrap.net/p2p/12D3KooWHBXCmXaUNat6ooynXG837JXPsZpSTeSzZx6DpgNatMmR",
            "/dnsaddr/da-bridge-5.celestia-bootstrap.net/p2p/12D3KooWDGTBK1a2Ru1qmnnRwP6Dmc44Zpsxi3xbgFk7ATEPfmEU",
            "/dnsaddr/da-bridge-6.celestia-bootstrap.net/p2p/12D3KooWLTUFyf3QEGqYkHWQS2yCtuUcL78vnKBdXU5gABM1YDeH",
            "/dnsaddr/da-full-1.celestia-bootstrap.net/p2p/12D3KooWKZCMcwGCYbL18iuw3YVpAZoyb1VBGbx9Kapsjw3soZgr",
            "/dnsaddr/da-full-2.celestia-bootstrap.net/p2p/12D3KooWE3fmRtHgfk9DCuQFfY3H3JYEnTU3xZozv1Xmo8KWrWbK",
            "/dnsaddr/da-full-3.celestia-bootstrap.net/p2p/12D3KooWK6Ftsd4XsWCsQZgZPNhTrE5urwmkoo5P61tGvnKmNVyv",
        ],
        Network::Arabica => &[
            "/dnsaddr/da-bridge-1.celestia-arabica-11.com/p2p/12D3KooWGqwzdEqM54Dce6LXzfFr97Bnhvm6rN7KM7MFwdomfm4S",
            "/dnsaddr/da-bridge-2.celestia-arabica-11.com/p2p/12D3KooWCMGM5eZWVfCN9ZLAViGfLUWAfXP5pCm78NFKb9jpBtua",
            "/dnsaddr/da-bridge-3.celestia-arabica-11.com/p2p/12D3KooWEWuqrjULANpukDFGVoHW3RoeUU53Ec9t9v5cwW3MkVdQ",
            "/dnsaddr/da-bridge-4.celestia-arabica-11.com/p2p/12D3KooWLT1ysSrD7XWSBjh7tU1HQanF5M64dHV6AuM6cYEJxMPk",
        ],
        Network::Mocha => &[
            "/dnsaddr/da-bridge-mocha-4.celestia-mocha.com/p2p/12D3KooWCBAbQbJSpCpCGKzqz3rAN4ixYbc63K68zJg9aisuAajg",
            "/dnsaddr/da-bridge-mocha-4-2.celestia-mocha.com/p2p/12D3KooWK6wJkScGQniymdWtBwBuU36n6BRXp9rCDDUD6P5gJr3G",
            "/dnsaddr/da-full-1-mocha-4.celestia-mocha.com/p2p/12D3KooWCUHPLqQXZzpTx1x3TAsdn3vYmTNDhzg66yG8hqoxGGN8",
            "/dnsaddr/da-full-2-mocha-4.celestia-mocha.com/p2p/12D3KooWR6SHsXPkkvhCRn6vp1RqSefgaT1X1nMNvrVjU2o3GoYy",
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
