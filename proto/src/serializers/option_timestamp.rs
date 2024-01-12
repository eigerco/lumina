//! [`serde`] serializer for the optional [`Timestamp`].

use celestia_tendermint_proto::google::protobuf::Timestamp;
use celestia_tendermint_proto::serializers::timestamp;
use serde::{Deserialize, Deserializer, Serializer};

/// Deserialize `Option<Timestamp>`.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Timestamp>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(transparent)]
    struct Def {
        #[serde(with = "timestamp")]
        value: Timestamp,
    }

    Ok(Option::<Def>::deserialize(deserializer)?.map(|def| def.value))
}

/// Serialize `Option<Timestamp>`.
pub fn serialize<S>(value: &Option<Timestamp>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(t) => timestamp::serialize(&t, serializer),
        None => serializer.serialize_none(),
    }
}
