//! Types related to creation and submission of blobs.

pub use celestia_tendermint_proto::v0_34::types::Blob as RawBlob;
use serde::{Deserialize, Serialize};

mod commitment;

pub use self::commitment::Commitment;
use crate::consts::appconsts::{subtree_root_threshold, AppVersion};
use crate::nmt::Namespace;
use crate::{bail_validation, Result, Share};

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
    /// Index of the blob's first share in the EDS. Only set for blobs retrieved from chain.
    // note: celestia supports deserializing blobs without index, so we should too
    #[serde(default, with = "index_serde")]
    pub index: Option<u64>,
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
    /// use celestia_types::{AppVersion, Blob, nmt::Namespace};
    ///
    /// let my_namespace = Namespace::new_v0(&[1, 2, 3, 4, 5]).expect("Invalid namespace");
    /// let blob = Blob::new(my_namespace, b"some data to store on blockchain".to_vec(), AppVersion::V1)
    ///     .expect("Failed to create a blob");
    ///
    /// assert_eq!(
    ///     &serde_json::to_string_pretty(&blob).unwrap(),
    ///     indoc::indoc! {r#"{
    ///       "namespace": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQIDBAU=",
    ///       "data": "c29tZSBkYXRhIHRvIHN0b3JlIG9uIGJsb2NrY2hhaW4=",
    ///       "share_version": 0,
    ///       "commitment": "m0A4feU6Fqd5Zy9td3M7lntG8A3PKqe6YdugmAsWz28=",
    ///       "index": -1
    ///     }"#},
    /// );
    /// ```
    pub fn new(namespace: Namespace, data: Vec<u8>, app_version: AppVersion) -> Result<Blob> {
        let subtree_root_threshold = subtree_root_threshold(app_version);
        let commitment = Commitment::from_blob(namespace, &data[..], 0, subtree_root_threshold)?;

        Ok(Blob {
            namespace,
            data,
            share_version: 0,
            commitment,
            index: None,
        })
    }

    /// Creates a `Blob` from [`RawBlob`] and an [`AppVersion`].
    pub fn from_raw(raw: RawBlob, app_version: AppVersion) -> Result<Blob> {
        let subtree_root_threshold = subtree_root_threshold(app_version);

        let namespace = Namespace::new(raw.namespace_version as u8, &raw.namespace_id)?;
        let commitment = Commitment::from_blob(
            namespace,
            &raw.data[..],
            raw.share_version as u8,
            subtree_root_threshold,
        )?;

        Ok(Blob {
            namespace,
            data: raw.data,
            share_version: raw.share_version as u8,
            commitment,
            index: None,
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
    /// # use celestia_types::consts::appconsts::AppVersion;
    /// # use celestia_types::nmt::Namespace;
    /// #
    /// # let namespace = Namespace::new_v0(&[1, 2, 3, 4, 5]).expect("Invalid namespace");
    ///
    /// let mut blob = Blob::new(namespace, b"foo".to_vec(), AppVersion::V1).unwrap();
    ///
    /// assert!(blob.validate(AppVersion::V1).is_ok());
    ///
    /// let other_blob = Blob::new(namespace, b"bar".to_vec(), AppVersion::V1).unwrap();
    /// blob.commitment = other_blob.commitment;
    ///
    /// assert!(blob.validate(AppVersion::V1).is_err());
    /// ```
    pub fn validate(&self, app_version: AppVersion) -> Result<()> {
        let subtree_root_threshold = subtree_root_threshold(app_version);

        let computed_commitment = Commitment::from_blob(
            self.namespace,
            &self.data,
            self.share_version,
            subtree_root_threshold,
        )?;

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
    /// # use celestia_types::consts::appconsts::AppVersion;
    /// # use celestia_types::nmt::Namespace;
    /// # let namespace = Namespace::new_v0(&[1, 2, 3, 4, 5]).expect("Invalid namespace");
    ///
    /// let blob = Blob::new(namespace, b"foo".to_vec(), AppVersion::V1).unwrap();
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

mod index_serde {
    use serde::ser::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize [`Option<u64>`] as `i64` with `None` represented as `-1`.
    pub fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let x = value
            .map(i64::try_from)
            .transpose()
            .map_err(S::Error::custom)?
            .unwrap_or(-1);
        serializer.serialize_i64(x)
    }

    /// Deserialize [`Option<u64>`] from `i64` with negative values as `None`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        i64::deserialize(deserializer).map(|val| if val >= 0 { Some(val as u64) } else { None })
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
              "commitment": "D6YGsPWdxR8ju2OcOspnkgPG2abD30pSHxsFdiPqnVk=",
              "index": -1
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn create_from_raw() {
        let expected = sample_blob();
        let raw = RawBlob::from(expected.clone());
        let created = Blob::from_raw(raw, AppVersion::V1).unwrap();

        assert_eq!(created, expected);
    }

    #[test]
    fn validate_blob() {
        sample_blob().validate(AppVersion::V1).unwrap();
    }

    #[test]
    fn validate_blob_commitment_mismatch() {
        let mut blob = sample_blob();
        blob.commitment.0.fill(7);

        blob.validate(AppVersion::V1).unwrap_err();
    }

    #[test]
    fn deserialize_blob_with_missing_index() {
        serde_json::from_str::<Blob>(
            r#"{
              "namespace": "AAAAAAAAAAAAAAAAAAAAAAAAAAAADCBNOWAP3dM=",
              "data": "8fIMqAB+kQo7+LLmHaDya8oH73hxem6lQWX1",
              "share_version": 0,
              "commitment": "D6YGsPWdxR8ju2OcOspnkgPG2abD30pSHxsFdiPqnVk="
            }"#,
        )
        .unwrap();
    }
}
