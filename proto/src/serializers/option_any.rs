use prost_types::Any;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Any>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Def {
        type_url: String,
        #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
        value: Vec<u8>,
    }

    let any = Option::<Def>::deserialize(deserializer)?.map(|def| Any {
        type_url: def.type_url,
        value: def.value,
    });

    Ok(any)
}

pub fn serialize<S>(value: &Option<Any>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    #[derive(Serialize)]
    struct Def<'a> {
        type_url: &'a str,
        #[serde(with = "tendermint_proto::serializers::bytes::base64string")]
        value: &'a [u8],
    }

    match value {
        Some(any) => {
            let def = Def {
                type_url: &any.type_url,
                value: &any.value,
            };

            def.serialize(serializer)
        }
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Any;

    #[derive(Serialize, Deserialize)]
    struct TxResponse {
        #[serde(default, with = "super")]
        tx: Option<Any>,
    }

    #[test]
    fn serialize() {
        let msg = TxResponse {
            tx: Some(Any {
                type_url: "abc".to_string(),
                value: vec![1, 2, 3],
            }),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"tx":{"type_url":"abc","value":"AQID"}}"#);
    }

    #[test]
    fn serialize_none() {
        let msg = TxResponse { tx: None };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"tx":null}"#);
    }

    #[test]
    fn deserialize() {
        let msg: TxResponse =
            serde_json::from_str(r#"{"tx":{"type_url":"abc","value":"AQID"}}"#).unwrap();
        let tx = msg.tx.unwrap();
        assert_eq!(tx.type_url, "abc");
        assert_eq!(tx.value, &[1, 2, 3])
    }

    #[test]
    fn deserialize_none() {
        let msg: TxResponse = serde_json::from_str(r#"{"tx":null}"#).unwrap();
        assert_eq!(msg.tx, None);

        let msg: TxResponse = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(msg.tx, None);
    }
}
