use std::fmt::Display;
use std::str::FromStr;

use cosmrs::AccountId;
use serde::{Deserialize, Serialize};
use tendermint::account::Id;

use crate::consts::cosmos::*;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    Account,
    Validator,
    Consensus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct Address {
    kind: AddressKind,
    id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct ValAddress {
    id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Raw", into = "Raw")]
pub struct AccAddress {
    id: Id,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
struct Raw {
    #[serde(with = "tendermint_proto::serializers::from_str")]
    addr: String,
}

impl Address {
    pub fn new(kind: AddressKind, bytes: &[u8]) -> Result<Self> {
        let bytes = bytes
            .try_into()
            .map_err(|_| Error::InvalidAddressSize(bytes.len()))?;
        let id = Id::new(bytes);

        Ok(Address { kind, id })
    }

    pub fn with_prefix(prefix: &str, bytes: &[u8]) -> Result<Self> {
        let kind = match prefix {
            BECH32_PREFIX_ACC_ADDR => AddressKind::Account,
            BECH32_PREFIX_VAL_ADDR => AddressKind::Validator,
            BECH32_PREFIX_CONS_ADDR => AddressKind::Consensus,
            s => return Err(Error::InvalidAddressPrefix(s.to_owned())),
        };

        Address::new(kind, bytes)
    }

    pub fn from_id(kind: AddressKind, id: Id) -> Self {
        Address { kind, id }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.id.as_bytes()
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn kind(&self) -> AddressKind {
        self.kind
    }

    pub fn prefix(&self) -> &'static str {
        match self.kind {
            AddressKind::Account => BECH32_PREFIX_ACC_ADDR,
            AddressKind::Validator => BECH32_PREFIX_VAL_ADDR,
            AddressKind::Consensus => BECH32_PREFIX_CONS_ADDR,
        }
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let account_id: AccountId = self.clone().into();
        f.write_str(account_id.as_ref())
    }
}

impl FromStr for Address {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let account_id = s.parse::<AccountId>()?;

        let kind = match account_id.prefix() {
            BECH32_PREFIX_ACC_ADDR => AddressKind::Account,
            BECH32_PREFIX_VAL_ADDR => AddressKind::Validator,
            BECH32_PREFIX_CONS_ADDR => AddressKind::Consensus,
            s => return Err(Error::InvalidAddressPrefix(s.to_owned())),
        };

        let bytes = account_id.to_bytes();

        Address::new(kind, &bytes)
    }
}

impl From<Address> for AccountId {
    fn from(addr: Address) -> AccountId {
        let prefix = addr.prefix();
        let bytes = addr.as_bytes();

        // There are two reasons that AccountId can return an error:
        //
        // 1. The prefix can have upper case characters
        // 2. The bytes length can be more than 255
        //
        // Both cases will never happen because we have full control at
        // the values.
        AccountId::new(prefix, bytes).expect("malformed prefix or bytes")
    }
}

impl TryFrom<AccountId> for Address {
    type Error = Error;

    fn try_from(account_id: AccountId) -> Result<Address, Error> {
        let prefix = account_id.prefix();
        let bytes = account_id.to_bytes();
        Address::with_prefix(prefix, &bytes)
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
        Raw {
            addr: value.to_string(),
        }
    }
}

impl From<Address> for Id {
    fn from(value: Address) -> Self {
        value.id
    }
}

impl PartialEq for Address {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialEq<Id> for Address {
    fn eq(&self, other: &Id) -> bool {
        self.id == *other
    }
}

impl PartialEq<AccAddress> for Address {
    fn eq(&self, other: &AccAddress) -> bool {
        self.id == other.id
    }
}

impl PartialEq<ValAddress> for Address {
    fn eq(&self, other: &ValAddress) -> bool {
        self.id == other.id
    }
}

impl AccAddress {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let bytes = bytes
            .try_into()
            .map_err(|_| Error::InvalidAddressSize(bytes.len()))?;
        let id = Id::new(bytes);

        Ok(AccAddress { id })
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.id.as_bytes()
    }

    pub fn id(&self) -> Id {
        self.id
    }
}

impl FromStr for AccAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let account_id = s.parse::<AccountId>()?;

        if account_id.prefix() != BECH32_PREFIX_ACC_ADDR {
            return Err(Error::InvalidAddressPrefix(account_id.prefix().to_owned()));
        }

        let bytes = account_id.to_bytes();

        AccAddress::new(&bytes)
    }
}

impl Display for AccAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let account_id: AccountId = self.clone().into();
        f.write_str(account_id.as_ref())
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
        Raw {
            addr: value.to_string(),
        }
    }
}

impl From<Id> for AccAddress {
    fn from(id: Id) -> Self {
        AccAddress { id }
    }
}

impl From<AccAddress> for AccountId {
    fn from(addr: AccAddress) -> AccountId {
        let prefix = BECH32_PREFIX_ACC_ADDR;
        let bytes = addr.as_bytes();
        AccountId::new(prefix, bytes).expect("malformed prefix or bytes")
    }
}

impl TryFrom<AccountId> for AccAddress {
    type Error = Error;

    fn try_from(account_id: AccountId) -> Result<AccAddress, Error> {
        if account_id.prefix() != BECH32_PREFIX_ACC_ADDR {
            return Err(Error::InvalidAddressPrefix(account_id.prefix().to_owned()));
        }

        let bytes = account_id.to_bytes();

        AccAddress::new(&bytes)
    }
}

impl From<AccAddress> for Id {
    fn from(value: AccAddress) -> Self {
        value.id
    }
}

impl PartialEq for AccAddress {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialEq<Id> for AccAddress {
    fn eq(&self, other: &Id) -> bool {
        self.id == *other
    }
}

impl PartialEq<Address> for AccAddress {
    fn eq(&self, other: &Address) -> bool {
        self.id == other.id
    }
}

impl PartialEq<ValAddress> for AccAddress {
    fn eq(&self, other: &ValAddress) -> bool {
        self.id == other.id
    }
}

impl ValAddress {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let bytes = bytes
            .try_into()
            .map_err(|_| Error::InvalidAddressSize(bytes.len()))?;
        let id = Id::new(bytes);

        Ok(ValAddress { id })
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.id.as_bytes()
    }

    pub fn id(&self) -> Id {
        self.id
    }
}

impl FromStr for ValAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let account_id = s.parse::<AccountId>()?;

        if account_id.prefix() != BECH32_PREFIX_VAL_ADDR {
            return Err(Error::InvalidAddressPrefix(account_id.prefix().to_owned()));
        }

        let bytes = account_id.to_bytes();

        ValAddress::new(&bytes)
    }
}

impl Display for ValAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let account_id: AccountId = self.clone().into();
        f.write_str(account_id.as_ref())
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
        Raw {
            addr: value.to_string(),
        }
    }
}

impl From<Id> for ValAddress {
    fn from(id: Id) -> Self {
        ValAddress { id }
    }
}

impl From<ValAddress> for AccountId {
    fn from(addr: ValAddress) -> AccountId {
        let prefix = BECH32_PREFIX_VAL_ADDR;
        let bytes = addr.as_bytes();
        AccountId::new(prefix, bytes).expect("malformed prefix or bytes")
    }
}

impl TryFrom<AccountId> for ValAddress {
    type Error = Error;

    fn try_from(account_id: AccountId) -> Result<ValAddress, Error> {
        if account_id.prefix() != BECH32_PREFIX_VAL_ADDR {
            return Err(Error::InvalidAddressPrefix(account_id.prefix().to_owned()));
        }

        let bytes = account_id.to_bytes();

        ValAddress::new(&bytes)
    }
}

impl From<ValAddress> for Id {
    fn from(value: ValAddress) -> Self {
        value.id
    }
}

impl PartialEq for ValAddress {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialEq<Id> for ValAddress {
    fn eq(&self, other: &Id) -> bool {
        self.id == *other
    }
}

impl PartialEq<Address> for ValAddress {
    fn eq(&self, other: &Address) -> bool {
        self.id == other.id
    }
}

impl PartialEq<AccAddress> for ValAddress {
    fn eq(&self, other: &AccAddress) -> bool {
        self.id == other.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDR1: [u8; 20] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    ];
    const ADDR1_ACC_STR: &str = "celestia1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5wgawu3";
    const ADDR1_VAL_STR: &str = "celestiavaloper1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5thlh2h";

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
    }

    #[test]
    fn parse_invalid_addr() {
        // Account address of 1 byte
        let addr = "celestia1qyu009tf";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();

        // Validator address of 1 byte
        let addr = "celestiavaloper1qy2jc8nq";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();

        // Unknown prefix
        let addr = "foobar1qypqxpq9qcrsszg2pvxq6rs0zqg3yyc5avgsnn";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();

        // Malformed string
        let addr = "asdsdfsdgsfd";
        addr.parse::<Address>().unwrap_err();
        addr.parse::<AccAddress>().unwrap_err();
        addr.parse::<ValAddress>().unwrap_err();
    }

    #[test]
    fn compare() {
        let addr: Address = ADDR1_ACC_STR.parse().unwrap();
        let acc_addr: AccAddress = ADDR1_ACC_STR.parse().unwrap();
        let val_addr: ValAddress = ADDR1_VAL_STR.parse().unwrap();
        let id = Id::new(ADDR1);

        assert_eq!(&addr, &acc_addr);
        assert_eq!(&addr, &val_addr);
        assert_eq!(&addr, &id);
        assert_eq!(&acc_addr, &addr);
        assert_eq!(&acc_addr, &val_addr);
        assert_eq!(&acc_addr, &id);
        assert_eq!(&val_addr, &addr);
        assert_eq!(&val_addr, &acc_addr);
        assert_eq!(&val_addr, &id);
    }
}
