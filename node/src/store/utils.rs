use celestia_types::ExtendedHeader;
use lumina_utils::executor::yield_now;
#[cfg(not(target_arch = "wasm32"))]
use tendermint_proto::Protobuf;

use crate::store::Result;
#[cfg(not(target_arch = "wasm32"))]
use crate::store::{SamplingMetadata, StoreError};

pub(crate) const VALIDATIONS_PER_YIELD: usize = 4;

/// Span of header that's been verified internally
#[derive(Clone)]
pub struct VerifiedExtendedHeaders(Vec<ExtendedHeader>);

impl IntoIterator for VerifiedExtendedHeaders {
    type Item = ExtendedHeader;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> TryFrom<&'a [ExtendedHeader]> for VerifiedExtendedHeaders {
    type Error = celestia_types::Error;

    fn try_from(value: &'a [ExtendedHeader]) -> Result<Self, Self::Error> {
        value.to_vec().try_into()
    }
}

impl From<VerifiedExtendedHeaders> for Vec<ExtendedHeader> {
    fn from(value: VerifiedExtendedHeaders) -> Self {
        value.0
    }
}

impl AsRef<[ExtendedHeader]> for VerifiedExtendedHeaders {
    fn as_ref(&self) -> &[ExtendedHeader] {
        &self.0
    }
}

/// 1-length header span is internally verified, this is valid
impl From<[ExtendedHeader; 1]> for VerifiedExtendedHeaders {
    fn from(value: [ExtendedHeader; 1]) -> Self {
        Self(value.into())
    }
}

impl From<ExtendedHeader> for VerifiedExtendedHeaders {
    fn from(value: ExtendedHeader) -> Self {
        Self(vec![value])
    }
}

impl From<&ExtendedHeader> for VerifiedExtendedHeaders {
    fn from(value: &ExtendedHeader) -> Self {
        Self(vec![value.to_owned()])
    }
}

impl TryFrom<Vec<ExtendedHeader>> for VerifiedExtendedHeaders {
    type Error = celestia_types::Error;

    fn try_from(headers: Vec<ExtendedHeader>) -> Result<Self, Self::Error> {
        let Some(head) = headers.first() else {
            return Ok(VerifiedExtendedHeaders(Vec::default()));
        };

        head.verify_adjacent_range(&headers[1..])?;

        Ok(Self(headers))
    }
}

impl VerifiedExtendedHeaders {
    /// Create a new instance out of pre-checked vec of headers
    ///
    /// # Safety
    ///
    /// This function may produce invalid `VerifiedExtendedHeaders`, if passed range is not
    /// validated manually
    pub unsafe fn new_unchecked(headers: Vec<ExtendedHeader>) -> Self {
        Self(headers)
    }
}

#[allow(unused)]
pub(crate) async fn validate_headers(headers: &[ExtendedHeader]) -> celestia_types::Result<()> {
    for headers in headers.chunks(VALIDATIONS_PER_YIELD) {
        for header in headers {
            header.validate()?;
        }

        // Validation is computation heavy so we yield on every chunk
        yield_now().await;
    }

    Ok(())
}

/// Deserializes [`SamplingMetadata`] and returns [`StoreError::StoredDataError`] on failure.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn deserialize_sampling_metadata(bytes: &[u8]) -> Result<SamplingMetadata> {
    SamplingMetadata::decode(bytes).map_err(|e| {
        let s = format!("Stored SamplingMetadata cannot be deserialized: {e}");
        StoreError::StoredDataError(s)
    })
}

/// Deserializes [`ExtendedHeader`] and returns [`StoreError::StoredDataError`] on failure.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn deserialize_extended_header(bytes: &[u8]) -> Result<ExtendedHeader> {
    ExtendedHeader::decode(bytes).map_err(|e| {
        let s = format!("Stored ExtendedHeader cannot be deserialized: {e}");
        StoreError::StoredDataError(s)
    })
}
