//! Primitives and constants related to the networks supported by Celestia nodes.

use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Supported Celestia networks.
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkId {
    /// The network identifier string
    pub id: String,
}

impl NetworkId {
    /// Creates validated network id.
    pub fn new(id: &str) -> Result<NetworkId, InvalidNetworkId> {
        if id.contains('/') {
            Err(InvalidNetworkId(id.to_owned()))
        } else {
            Ok(NetworkId { id: id.to_owned() })
        }
    }
}

impl AsRef<str> for NetworkId {
    fn as_ref(&self) -> &str {
        &self.id
    }
}

impl Deref for NetworkId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.id
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
        f.write_str(&self.id)
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
            Network::Custom(s) => &s.id,
        }
    }

    /// Get official Celestia and Lumina bootnodes for the given network.
    pub fn canonical_bootnodes(&self) -> impl Iterator<Item = Multiaddr> {
        let peers: &[_] = match self {
            Network::Mainnet => &[
                "/dnsaddr/da-bootstrapper-1.celestia-bootstrap.net/p2p/12D3KooWSqZaLcn5Guypo2mrHr297YPJnV8KMEMXNjs3qAS8msw8",
                "/dnsaddr/da-bootstrapper-2.celestia-bootstrap.net/p2p/12D3KooWQpuTFELgsUypqp9N4a1rKBccmrmQVY8Em9yhqppTJcXf",
                "/dnsaddr/da-bootstrapper-3.celestia-bootstrap.net/p2p/12D3KooWKZCMcwGCYbL18iuw3YVpAZoyb1VBGbx9Kapsjw3soZgr",
                "/dnsaddr/da-bootstrapper-4.celestia-bootstrap.net/p2p/12D3KooWE3fmRtHgfk9DCuQFfY3H3JYEnTU3xZozv1Xmo8KWrWbK",
                "/dnsaddr/boot.celestia.pops.one/p2p/12D3KooWBBzzGy5hAHUQVh2vBvL25CKwJ7wbmooPcz4amQhzJHJq",
                "/dnsaddr/celestia.qubelabs.io/p2p/12D3KooWAzucnC7yawvLbmVxv53ihhjbHFSVZCsPuuSzTg6A7wgx",
                "/dnsaddr/celestia-bootstrapper.binary.builders/p2p/12D3KooWDKvTzMnfh9j7g4RpvU6BXwH3AydTrzr1HTW6TMBQ61HF",
            ],
            Network::Arabica => &[
                "/dnsaddr/da-bridge-1.celestia-arabica-11.com/p2p/12D3KooWGqwzdEqM54Dce6LXzfFr97Bnhvm6rN7KM7MFwdomfm4S",
                "/dnsaddr/da-full-1.celestia-arabica-11.com/p2p/12D3KooWCMGM5eZWVfCN9ZLAViGfLUWAfXP5pCm78NFKb9jpBtua",
            ],
            Network::Mocha => &[
                "/dnsaddr/da-bootstrapper-1-mocha-4.celestia-mocha.com/p2p/12D3KooWCBAbQbJSpCpCGKzqz3rAN4ixYbc63K68zJg9aisuAajg",
                "/dnsaddr/da-bootstrapper-2-mocha-4.celestia-mocha.com/p2p/12D3KooWCUHPLqQXZzpTx1x3TAsdn3vYmTNDhzg66yG8hqoxGGN8",
                "/dnsaddr/mocha-boot.pops.one/p2p/12D3KooWDzNyDSvTBdKQAmnsUdAyQCQWwM3ReXTmPaaf6LzfNwRs",
                "/dnsaddr/celestia-mocha.qubelabs.io/p2p/12D3KooWQVmHy7JpfxpKZfLjvn12GjvMgKrWdsHkFbV2kKqQFBCG",
                "/dnsaddr/celestia-mocha4-bootstrapper.binary.builders/p2p/12D3KooWK6AYaPSe2EP99NP5G2DKwWLfMi6zHMYdD65KRJwdJSVU",
                "/dnsaddr/celestia-testnet-boot.01node.com/p2p/12D3KooWR923Tc8SCzweyaGZ5VU2ahyS9VWrQ8mDz56RbHjHFdzW",
                "/dnsaddr/celestia-mocha-boot.zkv.xyz/p2p/12D3KooWFdkhm7Ac6nqNkdNiW2g16KmLyyQrqXMQeijdkwrHqQ9J",
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
            Network::Custom(s) => s,
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
