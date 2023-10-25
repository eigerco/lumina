use std::str::FromStr;

use celestia_types::hash::Hash;
#[cfg(not(target_arch = "wasm32"))]
use clap::ValueEnum;
use hex::FromHexError;
use libp2p::core::multiaddr::Error as MultiaddrError;
use libp2p::Multiaddr;
use serde_repr::{Deserialize_repr, Serialize_repr};
use thiserror::Error;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg_attr(not(target_arch = "wasm32"), derive(ValueEnum))]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum Network {
    Arabica,
    Mocha,
    #[default]
    Private,
}

#[derive(Debug, Error)]
#[error("unknown network {0}")]
pub struct UnknownNetworkError(String);

#[derive(Debug, Error)]
pub enum NetworkGenesisError {
    #[error("error decoding genesis hash: {0}")]
    HexDecodeError(#[from] FromHexError),
    #[error("decoded genesis has invalid length: {0}")]
    InvalidLengthError(usize),
}

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

pub fn network_genesis(network: Network) -> Result<Option<Hash>, NetworkGenesisError> {
    let hex = match network {
        Network::Arabica => "5904E55478BA4B3002EE885621E007A2A6A2399662841912219AECD5D5CBE393",
        Network::Mocha => "B93BBE20A0FBFDF955811B6420F8433904664D45DB4BF51022BE4200C1A1680D",
        Network::Private => return Ok(None),
    };

    let bytes = hex::decode(hex)?;
    let array = bytes
        .try_into()
        .map_err(|b: Vec<u8>| NetworkGenesisError::InvalidLengthError(b.len()))?;

    Ok(Some(Hash::Sha256(array)))
}

pub fn canonical_network_bootnodes(network: Network) -> Result<Vec<Multiaddr>, MultiaddrError> {
    match network {
        Network::Arabica => [
                "/dns4/da-bridge.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWM3e9MWtyc8GkP8QRt74Riu17QuhGfZMytB2vq5NwkWAu",
                "/dns4/da-bridge-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWKj8mcdiBGxQRe1jqhaMnh2tGoC3rPDmr5UH2q8H4WA9M",
                "/dns4/da-full-1.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWBWkgmN7kmJSFovVrCjkeG47FkLGq7yEwJ2kEqNKCsBYk",
                "/dns4/da-full-2.celestia-arabica-10.com/tcp/2121/p2p/12D3KooWRByRF67a2kVM2j4MP5Po3jgTw7H2iL2Spu8aUwPkrRfP",
            ]
            .iter()
            .map(|s| s.parse())
            .collect()
        ,
        Network::Mocha => [
            "/dns4/da-bridge-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWCBAbQbJSpCpCGKzqz3rAN4ixYbc63K68zJg9aisuAajg",
            "/dns4/da-bridge-mocha-4-2.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWK6wJkScGQniymdWtBwBuU36n6BRXp9rCDDUD6P5gJr3G",
            "/dns4/da-full-1-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWCUHPLqQXZzpTx1x3TAsdn3vYmTNDhzg66yG8hqoxGGN8",
            "/dns4/da-full-2-mocha-4.celestia-mocha.com/udp/2121/quic/p2p/12D3KooWR6SHsXPkkvhCRn6vp1RqSefgaT1X1nMNvrVjU2o3GoYy",
            ]
            .iter()
            .map(|s| s.parse())
            .collect()
        ,
        Network::Private => Ok(vec![])
    }
}
