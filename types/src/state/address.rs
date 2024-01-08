use std::fmt::Display;
use std::str::FromStr;

use bech32::{FromBase32, ToBase32};
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};
use tendermint::account::Id;

use crate::consts::cosmos::*;
use crate::{Error, Result};

/// A generic representation of addresses in Celestia network.
#[enum_dispatch(Address)]
pub trait AddressTrait: FromStr + Display + private::Sealed {
    /// Get a reference to the account's ID.
    fn id_ref(&self) -> &Id;
    /// Get the kind of address.
    fn kind(&self) -> AddressKind;

    /// Get the account's ID.
    #[inline]
    fn id(&self) -> Id {
        *self.id_ref()
    }

    /// Convert the address to a byte slice.
    #[inline]
    fn as_bytes(&self) -> &[u8] {
        self.id_ref().as_bytes()
    }

    /// Get a `bech32` human readable prefix of the account kind.
    #[inline]
    fn prefix(&self) -> &'static str {
        self.kind().prefix()
    }
}

mod private {
    pub trait Sealed {}
}

/// Different kinds of addresses supported by Celestia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    /// Account address kind.
    Account,
    /// Validator address kind.
    Validator,
    /// Consensus address kind.
    Consensus,
}

/// A Celestia address. Either account, consensus or validator.
#[enum_dispatch]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub enum Address {
    /// Account address.
    AccAddress,
    /// Validator address.
    ValAddress,
    /// Consensus address.
    ConsAddress,
}

/// Address of an account.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct AccAddress {
    id: Id,
}

/// Address of a validator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct ValAddress {
    id: Id,
}

/// Address of a consensus node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct ConsAddress {
    id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
struct Raw {
    #[serde(with = "tendermint_proto::serializers::from_str")]
    addr: String,
}

impl AddressKind {
    /// Get the `bech32` human readable prefix.
    pub fn prefix(&self) -> &'static str {
        match self {
            AddressKind::Account => BECH32_PREFIX_ACC_ADDR,
            AddressKind::Validator => BECH32_PREFIX_VAL_ADDR,
            AddressKind::Consensus => BECH32_PREFIX_CONS_ADDR,
        }
    }
}

impl FromStr for AddressKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            BECH32_PREFIX_ACC_ADDR => Ok(AddressKind::Account),
            BECH32_PREFIX_VAL_ADDR => Ok(AddressKind::Validator),
            BECH32_PREFIX_CONS_ADDR => Ok(AddressKind::Consensus),
            _ => Err(Error::InvalidAddressPrefix(s.to_owned())),
        }
    }
}

impl private::Sealed for Address {}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Address::AccAddress(v) => <AccAddress as Display>::fmt(v, f),
            Address::ValAddress(v) => <ValAddress as Display>::fmt(v, f),
            Address::ConsAddress(v) => <ConsAddress as Display>::fmt(v, f),
        }
    }
}

impl FromStr for Address {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = string_to_kind_and_id(s)?;

        match kind {
            AddressKind::Account => Ok(AccAddress::new(id).into()),
            AddressKind::Validator => Ok(ValAddress::new(id).into()),
            AddressKind::Consensus => Ok(ConsAddress::new(id).into()),
        }
    }
}

impl TryFrom<Raw> for Address {
    type Error = Error;

    fn try_from(value: Raw) -> Result<Self, Self::Error> {
        value.addr.parse()
    }
}

impl From<Address> for Raw {
    fn from(value: Address) -> Self {
        let addr = value.to_string();
        Raw { addr }
    }
}

impl AccAddress {
    /// Create a new account address with given ID.
    pub fn new(id: Id) -> Self {
        AccAddress { id }
    }
}

impl AddressTrait for AccAddress {
    #[inline]
    fn id_ref(&self) -> &Id {
        &self.id
    }

    #[inline]
    fn kind(&self) -> AddressKind {
        AddressKind::Account
    }
}

impl private::Sealed for AccAddress {}

impl Display for AccAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = address_to_string(self);
        f.write_str(&s)
    }
}

impl FromStr for AccAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = string_to_kind_and_id(s)?;

        match kind {
            AddressKind::Account => Ok(AccAddress::new(id)),
            _ => Err(Error::InvalidAddressPrefix(kind.prefix().to_owned())),
        }
    }
}

impl TryFrom<Raw> for AccAddress {
    type Error = Error;

    fn try_from(value: Raw) -> Result<Self, Self::Error> {
        value.addr.parse()
    }
}

impl From<AccAddress> for Raw {
    fn from(value: AccAddress) -> Self {
        let addr = value.to_string();
        Raw { addr }
    }
}

impl ValAddress {
    /// Create a new validator address with given ID.
    pub fn new(id: Id) -> Self {
        ValAddress { id }
    }
}

impl AddressTrait for ValAddress {
    #[inline]
    fn id_ref(&self) -> &Id {
        &self.id
    }

    #[inline]
    fn kind(&self) -> AddressKind {
        AddressKind::Validator
    }
}

impl private::Sealed for ValAddress {}

impl Display for ValAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = address_to_string(self);
        f.write_str(&s)
    }
}

impl FromStr for ValAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = string_to_kind_and_id(s)?;

        match kind {
            AddressKind::Validator => Ok(ValAddress::new(id)),
            _ => Err(Error::InvalidAddressPrefix(kind.prefix().to_owned())),
        }
    }
}

impl TryFrom<Raw> for ValAddress {
    type Error = Error;

    fn try_from(value: Raw) -> Result<Self, Self::Error> {
        value.addr.parse()
    }
}

impl From<ValAddress> for Raw {
    fn from(value: ValAddress) -> Self {
        let addr = value.to_string();
        Raw { addr }
    }
}

impl ConsAddress {
    /// Create a new consensus address with given ID.
    pub fn new(id: Id) -> Self {
        ConsAddress { id }
    }
}

impl AddressTrait for ConsAddress {
    #[inline]
    fn id_ref(&self) -> &Id {
        &self.id
    }

    #[inline]
    fn kind(&self) -> AddressKind {
        AddressKind::Consensus
    }
}

impl private::Sealed for ConsAddress {}

impl Display for ConsAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = address_to_string(self);
        f.write_str(&s)
    }
}

impl FromStr for ConsAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = string_to_kind_and_id(s)?;

        match kind {
            AddressKind::Consensus => Ok(ConsAddress::new(id)),
            _ => Err(Error::InvalidAddressPrefix(kind.prefix().to_owned())),
        }
    }
}

impl TryFrom<Raw> for ConsAddress {
    type Error = Error;

    fn try_from(value: Raw) -> Result<Self, Self::Error> {
        value.addr.parse()
    }
}

impl From<ConsAddress> for Raw {
    fn from(value: ConsAddress) -> Self {
        let addr = value.to_string();
        Raw { addr }
    }
}

fn address_to_string(addr: &impl AddressTrait) -> String {
    let data_u5 = addr.as_bytes().to_base32();

    // We have full control on the prefix, so we know this will not fail
    bech32::encode(addr.prefix(), data_u5, bech32::Variant::Bech32).expect("Invalid prefix")
}

fn string_to_kind_and_id(s: &str) -> Result<(AddressKind, Id)> {
    let (hrp, data_u5, _) = bech32::decode(s).map_err(|_| Error::InvalidAddress(s.to_owned()))?;
    let data = Vec::from_base32(&data_u5).map_err(|_| Error::InvalidAddress(s.to_owned()))?;

    let kind = hrp.parse()?;
    let bytes = data[..]
        .try_into()
        .map_err(|_| Error::InvalidAddressSize(data.len()))?;

    Ok((kind, Id::new(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    const ADDR1: [u8; 20] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    ];
    const ADDR1_ACC_STR: &str = "celestia1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5wgawu3";
    const ADDR1_VAL_STR: &str = "celestiavaloper1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5thlh2h";
    const ADDR1_CONS_STR: &str = "celestiavalcons1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5lyvtxk";

    #[test]
    fn parse_acc_addr() {
        let addr: Address = ADDR1_ACC_STR.parse().unwrap();
        assert_eq!(addr.kind(), AddressKind::Account);
        assert_eq!(addr.as_bytes(), ADDR1);
        assert_eq!(&addr.to_string(), ADDR1_ACC_STR);

        let addr: AccAddress = ADDR1_ACC_STR.parse().unwrap();
        assert_eq!(addr.as_bytes(), ADDR1);
        assert_eq!(&addr.to_string(), ADDR1_ACC_STR);

        ADDR1_ACC_STR.parse::<ValAddress>().unwrap_err();
        ADDR1_ACC_STR.parse::<ConsAddress>().unwrap_err();
    }

    #[test]
    fn serde_acc_addr() {
        let addr_json = format!("\"{ADDR1_ACC_STR}\"");

        let addr: Address = serde_json::from_str(&addr_json).unwrap();
        assert_eq!(addr.kind(), AddressKind::Account);
        assert_eq!(addr.as_bytes(), ADDR1);

        let addr: AccAddress = serde_json::from_str(&addr_json).unwrap();
        assert_eq!(addr.as_bytes(), ADDR1);

        serde_json::from_str::<ValAddress>(&addr_json).unwrap_err();
        serde_json::from_str::<ConsAddress>(&addr_json).unwrap_err();
    }

    #[test]
    fn parse_val_addr() {
        let addr: Address = ADDR1_VAL_STR.parse().unwrap();
        assert_eq!(addr.kind(), AddressKind::Validator);
        assert_eq!(addr.as_bytes(), ADDR1);
        assert_eq!(&addr.to_string(), ADDR1_VAL_STR);

        let addr: ValAddress = ADDR1_VAL_STR.parse().unwrap();
        assert_eq!(addr.as_bytes(), ADDR1);
        assert_eq!(&addr.to_string(), ADDR1_VAL_STR);

        ADDR1_VAL_STR.parse::<AccAddress>().unwrap_err();
        ADDR1_VAL_STR.parse::<ConsAddress>().unwrap_err();
    }

    #[test]
    fn serde_val_addr() {
        let addr_json = format!("\"{ADDR1_VAL_STR}\"");

        let addr: Address = serde_json::from_str(&addr_json).unwrap();
        assert_eq!(addr.kind(), AddressKind::Validator);
        assert_eq!(addr.as_bytes(), ADDR1);

        let addr: ValAddress = serde_json::from_str(&addr_json).unwrap();
        assert_eq!(addr.as_bytes(), ADDR1);

        serde_json::from_str::<AccAddress>(&addr_json).unwrap_err();
        serde_json::from_str::<ConsAddress>(&addr_json).unwrap_err();
    }

    #[test]
    fn parse_cons_addr() {
        let addr: Address = ADDR1_CONS_STR.parse().unwrap();
        assert_eq!(addr.kind(), AddressKind::Consensus);
        assert_eq!(addr.as_bytes(), ADDR1);
        assert_eq!(&addr.to_string(), ADDR1_CONS_STR);

        let addr: ConsAddress = ADDR1_CONS_STR.parse().unwrap();
        assert_eq!(addr.as_bytes(), ADDR1);
        assert_eq!(&addr.to_string(), ADDR1_CONS_STR);

        ADDR1_CONS_STR.parse::<AccAddress>().unwrap_err();
        ADDR1_CONS_STR.parse::<ValAddress>().unwrap_err();
    }

    #[test]
    fn serde_cons_addr() {
        let addr_json = format!("\"{ADDR1_CONS_STR}\"");

        let addr: Address = serde_json::from_str(&addr_json).unwrap();
        assert_eq!(addr.kind(), AddressKind::Consensus);
        assert_eq!(addr.as_bytes(), ADDR1);

        let addr: ConsAddress = serde_json::from_str(&addr_json).unwrap();
        assert_eq!(addr.as_bytes(), ADDR1);

        serde_json::from_str::<AccAddress>(&addr_json).unwrap_err();
        serde_json::from_str::<ValAddress>(&addr_json).unwrap_err();
    }

    #[test]
    fn parse_invalid_addr() {
        // Account address of 1 byte
        let addr = "celestia1qyu009tf";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();
        addr.parse::<ConsAddress>().unwrap_err();

        // Validator address of 1 byte
        let addr = "celestiavaloper1qy2jc8nq";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();
        addr.parse::<ConsAddress>().unwrap_err();

        // Consensus address of 1 byte
        let addr = "celestiavalcons1qy2zlull";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();
        addr.parse::<ConsAddress>().unwrap_err();

        // Unknown prefix
        let addr = "foobar1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5avgsnn";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();
        addr.parse::<ConsAddress>().unwrap_err();

        // Malformed string
        let addr = "asdsdfsdgsfd";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();
        addr.parse::<ConsAddress>().unwrap_err();
    }

    #[test]
    fn convert() {
        let addr: Address = ADDR1_ACC_STR.parse().unwrap();
        let acc_addr: AccAddress = addr.try_into().unwrap();
        let _addr: Address = acc_addr.into();

        let addr: Address = ADDR1_VAL_STR.parse().unwrap();
        let val_addr: ValAddress = addr.try_into().unwrap();
        let _addr: Address = val_addr.into();

        let addr: Address = ADDR1_CONS_STR.parse().unwrap();
        let cons_addr: ConsAddress = addr.try_into().unwrap();
        let _addr: Address = cons_addr.into();
    }
}
