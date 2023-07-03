//! Serialize/deserialize `Option<T>`, where `None` turns into empty struct.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(untagged)]
enum TypeOrEmpty<T> {
    T(T),
    Empty(Empty),
    Null,
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    match Deserialize::deserialize(deserializer)? {
        TypeOrEmpty::T(t) => Ok(Some(t)),
        TypeOrEmpty::Empty(_) | TypeOrEmpty::Null => Ok(None),
    }
}

pub fn serialize<S, T>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    match value {
        Some(value) => value.serialize(serializer),
        None => Empty {}.serialize(serializer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Row {
        #[serde(with = "super")]
        proof: Option<Proof>,
    }

    #[derive(Serialize, Deserialize)]
    struct Proof {
        start: u64,
    }

    #[test]
    fn deserialize() {
        let row: Row = serde_json::from_str(r#"{ "proof": { "start": 1234 } }"#).unwrap();
        assert_eq!(row.proof.unwrap().start, 1234);
    }

    #[test]
    fn deserialize_empty() {
        let row: Row = serde_json::from_str(r#"{ "proof": {} }"#).unwrap();
        assert!(row.proof.is_none());
    }

    #[test]
    fn deserialize_null() {
        let row: Row = serde_json::from_str(r#"{ "proof": null }"#).unwrap();
        assert!(row.proof.is_none());
    }

    #[test]
    fn serialize() {
        let json = serde_json::to_string(&Row {
            proof: Some(Proof { start: 1234 }),
        })
        .unwrap();
        assert_eq!(json, r#"{"proof":{"start":1234}}"#);
    }

    #[test]
    fn serialize_empty() {
        let json = serde_json::to_string(&Row { proof: None }).unwrap();
        assert_eq!(json, r#"{"proof":{}}"#);
    }
}
