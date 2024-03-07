//! Types related to creation and submission of blobs.

use celestia_tendermint_proto::v0_34::types::Blob as RawBlob;
use celestia_tendermint_proto::Protobuf;
use serde::{Deserialize, Serialize};

mod commitment;

pub use self::commitment::Commitment;
use crate::consts::appconsts;
use crate::nmt::Namespace;
use crate::{bail_validation, Error, Result, Share};

/// GasPrice represents the amount to be paid per gas unit.
///
/// Fee is set by multiplying GasPrice by GasLimit, which is determined by the blob sizes.
/// If no value is provided, then this will be serialized to `-1.0` which means the node that
/// receives the request will calculate the GasPrice for given blob.
/// Read more about the mechanisms of fees and gas usage in [`submitting data blobs`].
///
/// [`submitting data blobs`]: https://docs.celestia.org/developers/submit-data#fees-and-gas-limits
#[derive(Debug, Default, Copy, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GasPrice(#[serde(with = "gas_prize_serde")] Option<f64>);

impl From<f64> for GasPrice {
    fn from(value: f64) -> Self {
        Self(Some(value))
    }
}

impl From<Option<f64>> for GasPrice {
    fn from(value: Option<f64>) -> Self {
        Self(value)
    }
}

/// Arbitrary data that can be stored in the network within certain [`Namespace`].
// NOTE: We don't use the `serde(try_from)` pattern for this type
// becase JSON representation needs to have `commitment` field but
// Protobuf definition doesn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    /// A [`Namespace`] the [`Blob`] belongs to.
    pub namespace: Namespace,
    /// Data stored within the [`Blob`].
    #[serde(with = "celestia_tendermint_proto::serializers::bytes::base64string")]
    pub data: Vec<u8>,
    /// Version indicating the format in which [`Share`]s should be created from this [`Blob`].
    ///
    /// [`Share`]: crate::share::Share
    pub share_version: u8,
    /// A [`Commitment`] computed from the [`Blob`]s data.
    pub commitment: Commitment,
}

impl Blob {
    /// Create a new blob with the given data within the [`Namespace`].
    ///
    /// # Errors
    ///
    /// This function propagates any error from the [`Commitment`] creation.
    ///
    /// # Example
    ///
    /// ```
    /// use celestia_types::{Blob, nmt::Namespace};
    ///
    /// let my_namespace = Namespace::new_v0(&[1, 2, 3, 4, 5]).expect("Invalid namespace");
    /// let blob = Blob::new(my_namespace, b"some data to store on blockchain".to_vec())
    ///     .expect("Failed to create a blob");
    ///
    /// assert_eq!(
    ///     &serde_json::to_string_pretty(&blob).unwrap(),
    ///     indoc::indoc! {r#"{
    ///       "namespace": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQIDBAU=",
    ///       "data": "c29tZSBkYXRhIHRvIHN0b3JlIG9uIGJsb2NrY2hhaW4=",
    ///       "share_version": 0,
    ///       "commitment": "m0A4feU6Fqd5Zy9td3M7lntG8A3PKqe6YdugmAsWz28="
    ///     }"#},
    /// );
    /// ```
    pub fn new(namespace: Namespace, data: Vec<u8>) -> Result<Blob> {
        let commitment =
            Commitment::from_blob(namespace, appconsts::SHARE_VERSION_ZERO, &data[..])?;

        Ok(Blob {
            namespace,
            data,
            share_version: appconsts::SHARE_VERSION_ZERO,
            commitment,
        })
    }

    /// Validate [`Blob`]s data with the [`Commitment`] it has.
    ///
    /// # Errors
    ///
    /// If validation fails, this function will return an error with a reason of failure.
    ///
    /// # Example
    ///
    /// ```
    /// use celestia_types::Blob;
    /// # use celestia_types::nmt::Namespace;
    /// #
    /// # let namespace = Namespace::new_v0(&[1, 2, 3, 4, 5]).expect("Invalid namespace");
    ///
    /// let mut blob = Blob::new(namespace, b"foo".to_vec()).unwrap();
    ///
    /// assert!(blob.validate().is_ok());
    ///
    /// let other_blob = Blob::new(namespace, b"bar".to_vec()).unwrap();
    /// blob.commitment = other_blob.commitment;
    ///
    /// assert!(blob.validate().is_err());
    /// ```
    pub fn validate(&self) -> Result<()> {
        let computed_commitment =
            Commitment::from_blob(self.namespace, self.share_version, &self.data)?;

        if self.commitment != computed_commitment {
            bail_validation!("blob commitment != localy computed commitment")
        }

        Ok(())
    }

    /// Encode the blob into a sequence of shares.
    ///
    /// Check the [`Share`] documentation for more information about the share format.
    ///
    /// # Errors
    ///
    /// This function will return an error if [`InfoByte`] creation fails
    /// or the data length overflows [`u32`].
    ///
    /// # Example
    ///
    /// ```
    /// use celestia_types::Blob;
    /// # use celestia_types::nmt::Namespace;
    /// # let namespace = Namespace::new_v0(&[1, 2, 3, 4, 5]).expect("Invalid namespace");
    ///
    /// let blob = Blob::new(namespace, b"foo".to_vec()).unwrap();
    /// let shares = blob.to_shares().unwrap();
    ///
    /// assert_eq!(shares.len(), 1);
    /// ```
    ///
    /// [`Share`]: crate::share::Share
    /// [`InfoByte`]: crate::share::InfoByte
    pub fn to_shares(&self) -> Result<Vec<Share>> {
        commitment::split_blob_to_shares(self.namespace, self.share_version, &self.data)
    }
}

impl Protobuf<RawBlob> for Blob {}

impl TryFrom<RawBlob> for Blob {
    type Error = Error;

    fn try_from(value: RawBlob) -> Result<Self, Self::Error> {
        let namespace = Namespace::new(value.namespace_version as u8, &value.namespace_id)?;
        let commitment =
            Commitment::from_blob(namespace, value.share_version as u8, &value.data[..])?;

        Ok(Blob {
            commitment,
            namespace,
            data: value.data,
            share_version: value.share_version as u8,
        })
    }
}

impl From<Blob> for RawBlob {
    fn from(value: Blob) -> RawBlob {
        RawBlob {
            namespace_id: value.namespace.id().to_vec(),
            namespace_version: value.namespace.version() as u32,
            data: value.data,
            share_version: value.share_version as u32,
        }
    }
}

mod gas_prize_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize [`Option<f64>`] with `None` represented as `-1`
    pub fn serialize<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let x = value.unwrap_or(-1.);
        serializer.serialize_f64(x)
    }

    /// Deserialize [`Option<f64>`] with an error when the value is not present.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        f64::deserialize(deserializer).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    fn sample_blob() -> Blob {
        serde_json::from_str(
            r#"{
              "namespace": "AAAAAAAAAAAAAAAAAAAAAAAAAAAADCBNOWAP3dM=",
              "data": "8fIMqAB+kQo7+LLmHaDya8oH73hxem6lQWX1",
              "share_version": 0,
              "commitment": "D6YGsPWdxR8ju2OcOspnkgPG2abD30pSHxsFdiPqnVk="
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn create_from_raw() {
        let expected = sample_blob();
        let raw = RawBlob::from(expected.clone());
        let created = Blob::try_from(raw).unwrap();

        assert_eq!(created, expected);
    }

    #[test]
    fn validate_blob() {
        sample_blob().validate().unwrap();
    }

    #[test]
    fn validate_blob_commitment_mismatch() {
        let mut blob = sample_blob();
        blob.commitment.0.fill(7);

        blob.validate().unwrap_err();
    }
}
