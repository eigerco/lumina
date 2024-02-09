pub(crate) mod none_as_negative_one {
    use serde::Serializer;

    /// Serialize [`Option<u64>`] with `None` represented as `-1`
    pub(crate) fn serialize<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let x = value.unwrap_or(-1.);
        serializer.serialize_f64(x)
    }

    #[cfg(test)]
    mod tests {
        use serde::{Deserialize, Serialize};

        #[cfg(target_arch = "wasm32")]
        use wasm_bindgen_test::wasm_bindgen_test as test;

        #[derive(Serialize, Deserialize, PartialEq)]
        #[serde(transparent)]
        struct OptionF64(#[serde(serialize_with = "super::serialize")] Option<f64>);

        #[test]
        fn serialize_none_as_negative_one() {
            let x = OptionF64(None);
            let serialized = serde_json::to_string(&x).unwrap();

            assert_eq!(&serialized, "-1");
        }
    }
}
