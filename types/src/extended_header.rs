use celestia_proto::header::pb::ExtendedHeader as RawExtendedHeader;
use serde::{Deserialize, Serialize};
use tendermint::block::header::Header;
use tendermint::block::{Commit, Height};
use tendermint::chain::id::Id;
use tendermint::{validator, Time};
use tendermint_proto::Protobuf;

use crate::{DataAvailabilityHeader, Error, Result};

pub type Validator = validator::Info;
pub type ValidatorSet = validator::Set;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawExtendedHeader", into = "RawExtendedHeader")]
pub struct ExtendedHeader {
    pub header: Header,
    pub commit: Commit,
    pub validator_set: ValidatorSet,
    pub dah: DataAvailabilityHeader,
}

impl ExtendedHeader {
    pub fn chain_id(&self) -> &Id {
        &self.header.chain_id
    }

    pub fn height(&self) -> Height {
        self.header.height
    }

    pub fn time(&self) -> Time {
        self.header.time
    }

    pub fn validate(&self) -> Result<()> {
        todo!();
    }

    pub fn verify(&self) {
        todo!();
    }
}

impl Protobuf<RawExtendedHeader> for ExtendedHeader {}

impl TryFrom<RawExtendedHeader> for ExtendedHeader {
    type Error = Error;

    fn try_from(value: RawExtendedHeader) -> Result<Self, Self::Error> {
        let header = value.header.ok_or(Error::MissingHeader)?.try_into()?;
        let commit = value.commit.ok_or(Error::MissingCommit)?.try_into()?;
        let validator_set = value
            .validator_set
            .ok_or(Error::MissingValidatorSet)?
            .try_into()?;
        let dah = value
            .dah
            .ok_or(Error::MissingDataAvailabilityHeader)?
            .try_into()?;

        Ok(ExtendedHeader {
            header,
            commit,
            validator_set,
            dah,
        })
    }
}

impl From<ExtendedHeader> for RawExtendedHeader {
    fn from(value: ExtendedHeader) -> RawExtendedHeader {
        RawExtendedHeader {
            header: Some(value.header.into()),
            commit: Some(value.commit.into()),
            validator_set: Some(value.validator_set.into()),
            dah: Some(value.dah.into()),
        }
    }
}
