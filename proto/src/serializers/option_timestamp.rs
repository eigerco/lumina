use serde::{Deserialize, Deserializer, Serializer};
use tendermint_proto::google::protobuf::Timestamp;
use tendermint_proto::serializers::timestamp;

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

pub fn serialize<S>(value: &Option<Timestamp>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(t) => timestamp::serialize(&t, serializer),
        None => serializer.serialize_none(),
    }
}
