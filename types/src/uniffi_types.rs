//! Misc types used by uniffi

use bytes::Bytes as RawBytes;
use tendermint::chain::Id as TendermintChainId;
use tendermint::time::Time as TendermintTime;
use uniffi::Record;

use crate::error::UniffiConversionError;

/// Chain identifier (e.g. ‘gaia-9000’)
#[derive(Record)]
pub struct ChainId {
    id: String,
}

impl From<TendermintChainId> for ChainId {
    fn from(value: TendermintChainId) -> Self {
        ChainId {
            id: value.to_string(),
        }
    }
}

impl TryFrom<ChainId> for TendermintChainId {
    type Error = UniffiConversionError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        TendermintChainId::try_from(value.id)
            .map_err(|_| UniffiConversionError::InvalidChainIdLength)
    }
}

uniffi::custom_type!(TendermintChainId, ChainId, {
    remote,
    try_lift: |value| Ok(value.try_into()?),
    lower: |value| value.into()
});

/// Tendermint timestamp
#[derive(Record)]
pub struct Time {
    ts: i64,
    nanos: u32,
}

impl TryFrom<TendermintTime> for Time {
    type Error = UniffiConversionError;

    fn try_from(value: TendermintTime) -> Result<Self, Self::Error> {
        const NANOSECONDS_IN_SECOND: i128 = 1_000_000_000;
        let ts = value.unix_timestamp_nanos();
        let nanos: u32 = (ts % NANOSECONDS_IN_SECOND)
            .try_into()
            .expect("remainder to fit");
        let ts: i64 = (ts / NANOSECONDS_IN_SECOND)
            .try_into()
            .map_err(|_| UniffiConversionError::TimestampOutOfRange)?;
        Ok(Time { ts, nanos })
    }
}

impl TryFrom<Time> for TendermintTime {
    type Error = UniffiConversionError;

    fn try_from(value: Time) -> Result<Self, Self::Error> {
        let Time { ts, nanos } = value;
        TendermintTime::from_unix_timestamp(ts, nanos)
            .map_err(|_| UniffiConversionError::TimestampOutOfRange)
    }
}

uniffi::custom_type!(TendermintTime, Time, {
    remote,
    try_lift: |value| Ok(value.try_into()?),
    lower: |value| value.try_into().expect("valid data")
});

/// Array of bytes
#[derive(Record)]
pub struct Bytes {
    /// Stored bytes
    bytes: Vec<u8>,
}

impl From<Bytes> for RawBytes {
    fn from(value: Bytes) -> Self {
        value.bytes.into()
    }
}

impl From<RawBytes> for Bytes {
    fn from(value: RawBytes) -> Self {
        Bytes {
            bytes: value.into(),
        }
    }
}

uniffi::custom_type!(RawBytes, Bytes, {
    remote,
    try_lift: |value| Ok(value.into()),
    lower: |value| value.into()
});
