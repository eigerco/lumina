use anyhow::Result;

const DEFAULT: &str = r#"#[serde(default)]"#;
const SERIALIZED: &str = r#"#[derive(::serde::Deserialize, ::serde::Serialize)]"#;
const BASE64STRING: &str =
    r#"#[serde(with = "tendermint_proto::serializers::bytes::base64string")]"#;
const VEC_BASE64STRING: &str =
    r#"#[serde(with = "tendermint_proto::serializers::bytes::vec_base64string")]"#;
const EMPTY_AS_NONE: &str = r#"#[serde(with = "crate::serializers::empty_as_none")]"#;
const PASCAL_CASE: &str = r#"#[serde(rename_all = "PascalCase")]"#;

pub static CUSTOM_TYPE_ATTRIBUTES: &[(&str, &str)] = &[
    (".celestia.da.DataAvailabilityHeader", SERIALIZED),
    (".header.pb.ExtendedHeader", SERIALIZED),
    (".share.p2p.shrex.nd.Proof", SERIALIZED),
    (".share.p2p.shrex.nd.Row", SERIALIZED),
    (".share.p2p.shrex.nd.Row", PASCAL_CASE),
];

pub static CUSTOM_FIELD_ATTRIBUTES: &[(&str, &str)] = &[
    (
        ".celestia.da.DataAvailabilityHeader.row_roots",
        VEC_BASE64STRING,
    ),
    (
        ".celestia.da.DataAvailabilityHeader.column_roots",
        VEC_BASE64STRING,
    ),
    (".share.p2p.shrex.nd.Proof.nodes", VEC_BASE64STRING),
    (".share.p2p.shrex.nd.Proof.hashleaf", DEFAULT),
    (".share.p2p.shrex.nd.Proof.hashleaf", BASE64STRING),
    (".share.p2p.shrex.nd.Row.shares", VEC_BASE64STRING),
    (".share.p2p.shrex.nd.Row.proof", DEFAULT),
    (".share.p2p.shrex.nd.Row.proof", EMPTY_AS_NONE),
];

fn main() -> Result<()> {
    let mut config = prost_build::Config::new();

    for (type_path, attr) in CUSTOM_TYPE_ATTRIBUTES {
        config.type_attribute(type_path, attr);
    }

    for (field_path, attr) in CUSTOM_FIELD_ATTRIBUTES {
        config.field_attribute(field_path, attr);
    }

    config
        .include_file("mod.rs")
        .extern_path(".tendermint", "::tendermint_proto::v0_34")
        .compile_protos(
            &[
                "vendor/celestia/da/data_availability_header.proto",
                "vendor/header/pb/extended_header.proto",
                "vendor/share/p2p/shrexnd/pb/share.proto",
            ],
            &["vendor"],
        )?;

    Ok(())
}
