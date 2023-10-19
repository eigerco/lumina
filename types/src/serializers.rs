pub(crate) mod none_as_negative_one {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Deserialize [`Option<u64>`] with negative numbers represented as `None`
    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<i128>::deserialize(deserializer)?;
        let value = match value {
            Some(..=-1) => None,
            Some(x) => Some(u64::try_from(x).map_err(serde::de::Error::custom)?),
            None => None,
        };
        Ok(value)
    }

    /// Serialize [`Option<u64>`] with `None` represented as `-1`
    pub(crate) fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let x = value.map(|x| x as i128).unwrap_or(-1);
        serializer.serialize_i128(x)
    }

    #[cfg(test)]
    mod tests {
        #[cfg(not(target_arch = "wasm32"))]
        use proptest::prelude::*;
        use serde::{Deserialize, Serialize};

        #[cfg(target_arch = "wasm32")]
        use wasm_bindgen_test::wasm_bindgen_test as test;

        #[derive(Serialize, Deserialize, PartialEq, Eq)]
        #[serde(transparent)]
        struct OptionU64(#[serde(with = "super")] Option<u64>);

        #[test]
        fn serialize_none_as_negative_one() {
            let x = OptionU64(None);
            let serialized = serde_json::to_string(&x).unwrap();

            assert_eq!(&serialized, "-1");
        }

        #[cfg(not(target_arch = "wasm32"))]
        proptest! {
            #[test]
            fn deserialize_negative(x in i64::MIN..0) {
                let x = format!("{x}");
                let opt_u64: OptionU64 = serde_json::from_str(&x).unwrap();

                assert_eq!(opt_u64.0, None);
            }

            #[test]
            fn serialize_deserialize(x: Option<u64>) {
                let serialized = serde_json::to_string(&OptionU64(x)).unwrap();
                let deserialized: OptionU64 = serde_json::from_str(&serialized).unwrap();

                assert_eq!(x, deserialized.0);
            }
        }
    }
}
