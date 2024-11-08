//! A build script generating rust types from protobuf definitions.

use prost_types::FileDescriptorSet;

const SERIALIZED: &str = r#"#[derive(::serde::Deserialize, ::serde::Serialize)]"#;
const SERIALIZED_DEFAULT: &str =
    r#"#[derive(::serde::Deserialize, ::serde::Serialize)] #[serde(default)]"#;
const TRANSPARENT: &str = r#"#[serde(transparent)]"#;
const BASE64STRING: &str =
    r#"#[serde(with = "celestia_tendermint_proto::serializers::bytes::base64string")]"#;
const QUOTED: &str = r#"#[serde(with = "celestia_tendermint_proto::serializers::from_str")]"#;
const VEC_BASE64STRING: &str =
    r#"#[serde(with = "celestia_tendermint_proto::serializers::bytes::vec_base64string")]"#;
#[cfg(not(feature = "tonic"))]
const OPTION_ANY: &str = r#"#[serde(with = "crate::serializers::option_any")]"#;
const OPTION_TIMESTAMP: &str = r#"#[serde(with = "crate::serializers::option_timestamp")]"#;
const NULL_DEFAULT: &str = r#"#[serde(with = "crate::serializers::null_default")]"#;

#[rustfmt::skip]
static CUSTOM_TYPE_ATTRIBUTES: &[(&str, &str)] = &[
    (".celestia.core.v1.da.DataAvailabilityHeader", SERIALIZED_DEFAULT),
    (".celestia.blob.v1.MsgPayForBlobs", SERIALIZED_DEFAULT),
    (".cosmos.base.abci.v1beta1.ABCIMessageLog", SERIALIZED_DEFAULT),
    (".cosmos.base.abci.v1beta1.Attribute", SERIALIZED_DEFAULT),
    (".cosmos.base.abci.v1beta1.StringEvent", SERIALIZED_DEFAULT),
    (".cosmos.base.abci.v1beta1.TxResponse", SERIALIZED_DEFAULT),
    (".cosmos.base.v1beta1.Coin", SERIALIZED_DEFAULT),
    (".cosmos.base.query.v1beta1.PageResponse", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.QueryDelegationResponse", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.DelegationResponse", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.Delegation", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.QueryRedelegationsResponse", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.RedelegationResponse", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.Redelegation", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.RedelegationEntryResponse", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.RedelegationEntry", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.QueryUnbondingDelegationResponse", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.UnbondingDelegation", SERIALIZED_DEFAULT),
    (".cosmos.staking.v1beta1.UnbondingDelegationEntry", SERIALIZED_DEFAULT),
    (".header.pb.ExtendedHeader", SERIALIZED_DEFAULT),
    (".share.eds.byzantine.pb.BadEncoding", SERIALIZED_DEFAULT),
    (".share.eds.byzantine.pb.Share", SERIALIZED_DEFAULT),
    (".proof.pb.Proof", SERIALIZED_DEFAULT),
    (".shwap.AxisType", SERIALIZED),
    (".shwap.Row", SERIALIZED),
    (".shwap.RowNamespaceData", SERIALIZED_DEFAULT),
    (".shwap.Sample", SERIALIZED_DEFAULT),
    (".shwap.Share", SERIALIZED_DEFAULT),
    (".shwap.Share", TRANSPARENT),
];

#[rustfmt::skip]
static CUSTOM_FIELD_ATTRIBUTES: &[(&str, &str)] = &[
    (".celestia.core.v1.da.DataAvailabilityHeader.row_roots", VEC_BASE64STRING),
    (".celestia.core.v1.da.DataAvailabilityHeader.column_roots", VEC_BASE64STRING),
    #[cfg(not(feature = "tonic"))]
    (".cosmos.base.abci.v1beta1.TxResponse.tx", OPTION_ANY),
    (".cosmos.base.abci.v1beta1.TxResponse.logs", NULL_DEFAULT),
    (".cosmos.base.abci.v1beta1.TxResponse.events", NULL_DEFAULT),
    (".cosmos.base.query.v1beta1.PageResponse.next_key", BASE64STRING),
    (".cosmos.staking.v1beta1.RedelegationEntry.completion_time", OPTION_TIMESTAMP),
    (".cosmos.staking.v1beta1.UnbondingDelegationEntry.completion_time", OPTION_TIMESTAMP),
    (".share.eds.byzantine.pb.BadEncoding.axis", QUOTED),
    (".proof.pb.Proof.nodes", VEC_BASE64STRING),
    (".proof.pb.Proof.leaf_hash", BASE64STRING),
    (".shwap.RowNamespaceData.shares", NULL_DEFAULT),
    (".shwap.Share", BASE64STRING),
];

#[rustfmt::skip]
static EXTERN_PATHS: &[(&str, &str)] = &[
    (".tendermint", "::celestia_tendermint_proto::v0_34"),
    (".google.protobuf.Timestamp", "::celestia_tendermint_proto::google::protobuf::Timestamp"),
    (".google.protobuf.Duration", "::celestia_tendermint_proto::google::protobuf::Duration"),
    (".cosmos.tx.v1beta1.TxBody", "cosmos_sdk_proto::cosmos::tx::v1beta1::TxBody"),
    (".cosmos.tx.v1beta1.AuthInfo", "cosmos_sdk_proto::cosmos::tx::v1beta1::AuthInfo"),
];

const PROTO_FILES: &[&str] = &[
    "vendor/celestia/blob/v1/params.proto",
    "vendor/celestia/blob/v1/query.proto",
    "vendor/celestia/blob/v1/tx.proto",
    "vendor/celestia/core/v1/da/data_availability_header.proto",
    "vendor/cosmos/auth/v1beta1/auth.proto",
    "vendor/cosmos/auth/v1beta1/query.proto",
    "vendor/cosmos/base/abci/v1beta1/abci.proto",
    "vendor/cosmos/base/node/v1beta1/query.proto",
    "vendor/cosmos/base/tendermint/v1beta1/query.proto",
    "vendor/cosmos/base/v1beta1/coin.proto",
    "vendor/cosmos/crypto/ed25519/keys.proto",
    "vendor/cosmos/crypto/multisig/v1beta1/multisig.proto",
    "vendor/cosmos/crypto/secp256k1/keys.proto",
    "vendor/cosmos/staking/v1beta1/query.proto",
    "vendor/cosmos/tx/v1beta1/service.proto",
    "vendor/cosmos/tx/v1beta1/tx.proto",
    "vendor/go-header/p2p/pb/header_request.proto",
    "vendor/header/pb/extended_header.proto",
    "vendor/share/eds/byzantine/pb/share.proto",
    "vendor/share/shwap/p2p/bitswap/pb/bitswap.proto",
    "vendor/share/shwap/pb/shwap.proto",
    "vendor/tendermint/types/types.proto",
];

const INCLUDES: &[&str] = &["vendor", "vendor/nmt"];

fn main() {
    let fds = protox_compile();
    prost_build(fds);
    #[cfg(feature = "tonic")]
    tonic_build(protox_compile())
}

fn protox_compile() -> FileDescriptorSet {
    protox::compile(PROTO_FILES, INCLUDES).expect("protox failed to build")
}

fn prost_build(fds: FileDescriptorSet) {
    let mut config = prost_build::Config::new();

    for (type_path, attr) in CUSTOM_TYPE_ATTRIBUTES {
        config.type_attribute(type_path, attr);
    }

    for (field_path, attr) in CUSTOM_FIELD_ATTRIBUTES {
        config.field_attribute(field_path, attr);
    }

    for (proto_path, rust_path) in EXTERN_PATHS {
        config.extern_path(proto_path.to_string(), rust_path.to_string());
    }

    config
        .include_file("mod.rs")
        // Comments in Google's protobuf are causing issues with cargo-test
        .disable_comments([".google"])
        .compile_fds(fds)
        .expect("prost failed");
}

#[cfg(feature = "tonic")]
fn tonic_build(fds: FileDescriptorSet) {
    let buf_img = tempfile::NamedTempFile::new()
        .expect("should be able to create a temp file to hold the buf image file descriptor set");

    let mut prost_config = prost_build::Config::new();
    prost_config.enable_type_names();

    let mut tonic_config = tonic_build::configure()
        .build_client(true)
        .build_server(false)
        .client_mod_attribute(".", "#[cfg(not(target_arch=\"wasm32\"))]")
        .use_arc_self(true)
        // override prost-types with pbjson-types
        .compile_well_known_types(true)
        .extern_path(".google.protobuf", "::pbjson_types")
        .file_descriptor_set_path(buf_img.path())
        .skip_protoc_run();

    for (type_path, attr) in CUSTOM_TYPE_ATTRIBUTES {
        tonic_config = tonic_config.type_attribute(type_path, attr);
    }
    for (proto_path, rust_path) in EXTERN_PATHS {
        tonic_config = tonic_config.extern_path(proto_path, rust_path);
    }
    for (field_path, attr) in CUSTOM_FIELD_ATTRIBUTES {
        tonic_config = tonic_config.field_attribute(field_path, attr);
    }

    tonic_config
        .compile_fds_with_config(prost_config, fds)
        .expect("should be able to compile protobuf using tonic");

    let descriptor_set = std::fs::read(buf_img.path())
        .expect("the buf image/descriptor set must exist and be readable at this point");

    pbjson_build::Builder::new()
        .register_descriptors(&descriptor_set)
        .unwrap()
        .build(&[
            ".celestia_proto",
            ".celestia",
            ".cosmos",
            ".tendermint",
            ".google",
        ])
        .unwrap();
}
