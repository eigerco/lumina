//! A build script generating rust types from protobuf definitions.

use anyhow::Result;

const SERIALIZED: &str = r#"#[derive(::serde::Deserialize, ::serde::Serialize)] #[serde(default)]"#;
const BASE64STRING: &str =
    r#"#[serde(with = "tendermint_proto::serializers::bytes::base64string")]"#;
const QUOTED: &str = r#"#[serde(with = "tendermint_proto::serializers::from_str")]"#;
const VEC_BASE64STRING: &str =
    r#"#[serde(with = "tendermint_proto::serializers::bytes::vec_base64string")]"#;
const OPTION_ANY: &str = r#"#[serde(with = "crate::serializers::option_any")]"#;
const OPTION_TIMESTAMP: &str = r#"#[serde(with = "crate::serializers::option_timestamp")]"#;

#[rustfmt::skip]
static CUSTOM_TYPE_ATTRIBUTES: &[(&str, &str)] = &[
    (".celestia.da.DataAvailabilityHeader", SERIALIZED),
    (".celestia.blob.v1.MsgPayForBlobs", SERIALIZED),
    (".cosmos.base.abci.v1beta1.ABCIMessageLog", SERIALIZED),
    (".cosmos.base.abci.v1beta1.Attribute", SERIALIZED),
    (".cosmos.base.abci.v1beta1.StringEvent", SERIALIZED),
    (".cosmos.base.abci.v1beta1.TxResponse", SERIALIZED),
    (".cosmos.base.v1beta1.Coin", SERIALIZED),
    (".cosmos.base.query.v1beta1.PageResponse", SERIALIZED),
    (".cosmos.staking.v1beta1.QueryDelegationResponse", SERIALIZED),
    (".cosmos.staking.v1beta1.DelegationResponse", SERIALIZED),
    (".cosmos.staking.v1beta1.Delegation", SERIALIZED),
    (".cosmos.staking.v1beta1.QueryRedelegationsResponse", SERIALIZED),
    (".cosmos.staking.v1beta1.RedelegationResponse", SERIALIZED),
    (".cosmos.staking.v1beta1.Redelegation", SERIALIZED),
    (".cosmos.staking.v1beta1.RedelegationEntryResponse", SERIALIZED),
    (".cosmos.staking.v1beta1.RedelegationEntry", SERIALIZED),
    (".cosmos.staking.v1beta1.QueryUnbondingDelegationResponse", SERIALIZED),
    (".cosmos.staking.v1beta1.UnbondingDelegation", SERIALIZED),
    (".cosmos.staking.v1beta1.UnbondingDelegationEntry", SERIALIZED),
    (".header.pb.ExtendedHeader", SERIALIZED),
    (".share.eds.byzantine.pb.BadEncoding", SERIALIZED),
    (".share.eds.byzantine.pb.Share", SERIALIZED),
    (".proof.pb.Proof", SERIALIZED),
    (".share.p2p.shrex.nd.NamespaceRowResponse", SERIALIZED),
];

#[rustfmt::skip]
static CUSTOM_FIELD_ATTRIBUTES: &[(&str, &str)] = &[
    (".celestia.da.DataAvailabilityHeader.row_roots", VEC_BASE64STRING),
    (".celestia.da.DataAvailabilityHeader.column_roots", VEC_BASE64STRING),
    (".cosmos.base.abci.v1beta1.TxResponse.tx", OPTION_ANY),
    (".cosmos.base.query.v1beta1.PageResponse.next_key", BASE64STRING),
    (".cosmos.staking.v1beta1.RedelegationEntry.completion_time", OPTION_TIMESTAMP),
    (".cosmos.staking.v1beta1.UnbondingDelegationEntry.completion_time", OPTION_TIMESTAMP),
    (".share.eds.byzantine.pb.BadEncoding.axis", QUOTED),
    (".proof.pb.Proof.nodes", VEC_BASE64STRING),
    (".proof.pb.Proof.leaf_hash", BASE64STRING),
    (".share.p2p.shrex.nd.NamespaceRowResponse.shares", VEC_BASE64STRING),
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
        .extern_path(
            ".google.protobuf.Timestamp",
            "::tendermint_proto::google::protobuf::Timestamp",
        )
        .extern_path(
            ".google.protobuf.Duration",
            "::tendermint_proto::google::protobuf::Duration",
        )
        // Comments in Google's protobuf are causing issues with cargo-test
        .disable_comments([".google"])
        .compile_protos(
            &[
                "vendor/celestia/da/data_availability_header.proto",
                "vendor/celestia/blob/v1/tx.proto",
                "vendor/header/pb/extended_header.proto",
                "vendor/share/p2p/shrexnd/pb/share.proto",
                "vendor/share/eds/byzantine/pb/share.proto",
                "vendor/cosmos/base/v1beta1/coin.proto",
                "vendor/cosmos/base/abci/v1beta1/abci.proto",
                "vendor/cosmos/crypto/multisig/v1beta1/multisig.proto",
                "vendor/cosmos/staking/v1beta1/query.proto",
                "vendor/cosmos/tx/v1beta1/tx.proto",
                "vendor/go-header/p2p/pb/header_request.proto",
            ],
            &["vendor", "vendor/nmt"],
        )?;

    Ok(())
}
