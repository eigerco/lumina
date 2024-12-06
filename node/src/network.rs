//! Primitives and constants related to the networks supported by Celestia nodes.

use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Supported Celestia networks.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Network {
    /// Celestia mainnet.
    #[default]
    Mainnet,
    /// Arabica testnet.
    Arabica,
    /// Mocha testnet.
    Mocha,
    /// Custom network.
    Custom(NetworkId),
}

/// Error for invalid network id.
#[derive(Debug, Error)]
#[error("Invalid network id: {0}")]
pub struct InvalidNetworkId(String);

/// Valid network id
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkId(String);

impl NetworkId {
    /// Creates validated network id.
    pub fn new(id: &str) -> Result<NetworkId, InvalidNetworkId> {
        if id.contains('/') {
            Err(InvalidNetworkId(id.to_owned()))
        } else {
            Ok(NetworkId(id.to_owned()))
        }
    }
}

impl AsRef<str> for NetworkId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for NetworkId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl FromStr for NetworkId {
    type Err = InvalidNetworkId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        NetworkId::new(s)
    }
}

impl fmt::Display for NetworkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Network {
    /// Creates a `Network::Custom` value.
    pub fn custom(id: &str) -> Result<Network, InvalidNetworkId> {
        Ok(Network::Custom(NetworkId::new(id)?))
    }

    /// Returns true if value is `Network::Custom` variant.
    pub fn is_custom(&self) -> bool {
        matches!(self, Network::Custom(_))
    }

    /// Get the network id.
    pub fn id(&self) -> &str {
        match self {
            Network::Mainnet => "celestia",
            Network::Arabica => "arabica-11",
            Network::Mocha => "mocha-4",
            Network::Custom(ref s) => &s.0,
        }
    }

    /// Get official Celestia and Lumina bootnodes for the given network.
    pub fn canonical_bootnodes(&self) -> impl Iterator<Item = Multiaddr> {
        let peers: &[_] = match self {
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
            Network::Custom(_) => &[],
        };
        peers
            .iter()
            .map(|s| s.parse().expect("Invalid bootstrap address"))
    }
}

impl FromStr for Network {
    type Err = InvalidNetworkId;

    fn from_str(value: &str) -> Result<Self, InvalidNetworkId> {
        match value {
            "Mainnet" | "MainNet" | "mainnet" | "celestia" => Ok(Network::Mainnet),
            "Arabica" | "arabica" | "arabica-11" => Ok(Network::Arabica),
            "Mocha" | "mocha" | "mocha-4" => Ok(Network::Mocha),
            custom_id => Network::custom(custom_id),
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Network::Mainnet => "Mainnet",
            Network::Arabica => "Arabica",
            Network::Mocha => "Mocha",
            Network::Custom(ref s) => s,
        };

        f.write_str(s)
    }
}

impl<'de> Deserialize<'de> for Network {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for Network {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl TryFrom<String> for Network {
    type Error = InvalidNetworkId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl<'a> TryFrom<&'a String> for Network {
    type Error = InvalidNetworkId;

    fn try_from(value: &'a String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl<'a> TryFrom<&'a str> for Network {
    type Error = InvalidNetworkId;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_network_bootnodes() {
        // canonical_network_bootnodes works on const data, test it doesn't panic and the data is there
        let mainnet = Network::Mainnet.canonical_bootnodes();
        assert_ne!(mainnet.count(), 0);

        let arabica = Network::Arabica.canonical_bootnodes();
        assert_ne!(arabica.count(), 0);

        let mocha = Network::Mocha.canonical_bootnodes();
        assert_ne!(mocha.count(), 0);

        let id = NetworkId::new("private").unwrap();
        let private = Network::Custom(id).canonical_bootnodes();
        assert_eq!(private.count(), 0);
    }

    #[test]
    fn check_network_id() {
        Network::custom("foo").unwrap();
        Network::custom("foo/bar").unwrap_err();
    }
}
