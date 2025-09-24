# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.7.0...celestia-grpc-v0.8.0) - 2025-09-24

### Added

- *(grpc,client)* Allow creating celestia-client with read-only grpc ([#755](https://github.com/eigerco/lumina/pull/755))
- *(grpc)* [**breaking**] Merge TxClient and GrpcClient, add builder ([#712](https://github.com/eigerco/lumina/pull/712))
- *(types)* [**breaking**] singular `Blob::new` constructor ([#719](https://github.com/eigerco/lumina/pull/719))

### Other

- *(grpc)* remove patch version of dyn-clone ([#749](https://github.com/eigerco/lumina/pull/749))

## [0.7.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.6.1...celestia-grpc-v0.7.0) - 2025-09-08

### Added

- *(types)* [**breaking**] Add support for app v6 ([#733](https://github.com/eigerco/lumina/pull/733))
- *(grpc)* remove retries on insufficient fee and gas multiplier ([#731](https://github.com/eigerco/lumina/pull/731))
- *(grpc)* [**breaking**] expose whole node config instead just gas price ([#732](https://github.com/eigerco/lumina/pull/732))
- *(client,grpc)* make sure all returned futures are Send ([#729](https://github.com/eigerco/lumina/pull/729))

### Other

- *(client,grpc,types)* Make all the celestia-client types types Serialisable ([#734](https://github.com/eigerco/lumina/pull/734))

## [0.6.1](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.6.0...celestia-grpc-v0.6.1) - 2025-08-19

### Other

- updated the following local packages: celestia-proto, celestia-types, celestia-rpc, celestia-rpc

## [0.6.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.5.0...celestia-grpc-v0.6.0) - 2025-08-13

### Added

- *(proto,types,rpc)* [**breaking**] upgrade to celestia-node v0.25 ([#720](https://github.com/eigerco/lumina/pull/720))
- *(proto,types)* [**breaking**] Update protos and switch to tendermint v0.38 ([#707](https://github.com/eigerco/lumina/pull/707))
- [**breaking**] Implement `celestia-client` crate ([#682](https://github.com/eigerco/lumina/pull/682))

## [0.5.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.4.1...celestia-grpc-v0.5.0) - 2025-07-29

### Added

- *(grpc)* [**breaking**] Expose entire GrpcClient API plus required type changes  ([#655](https://github.com/eigerco/lumina/pull/655))
- *(grpc)* [**breaking**] Add support for Gas Estimation Service ([#680](https://github.com/eigerco/lumina/pull/680))
- *(grpc)* [**breaking**] Trustless balance queries ([#677](https://github.com/eigerco/lumina/pull/677))
- *(grpc,types,node)* [**breaking**] Wasm grpc client ([#654](https://github.com/eigerco/lumina/pull/654))
- *(grpc)* Streamline TxClient creation API, add docs with example ([#673](https://github.com/eigerco/lumina/pull/673))

## [0.4.1](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.4.0...celestia-grpc-v0.4.1) - 2025-07-02

### Other

- updated the following local packages: celestia-types

## [0.4.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.3.1...celestia-grpc-v0.4.0) - 2025-06-20

### Added

- *(grpc)* [**breaking**] add memo field to TxConfig ([#659](https://github.com/eigerco/lumina/pull/659))

## [0.3.1](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.3.0...celestia-grpc-v0.3.1) - 2025-06-09

### Added

- *(node-uniffi)* Add grpc types and client for uniffi ([#627](https://github.com/eigerco/lumina/pull/627))

## [0.3.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.2.2...celestia-grpc-v0.3.0) - 2025-05-26

### Added

- *(grpc)* expose DocSigner and IntoAny ([#604](https://github.com/eigerco/lumina/pull/604))

### Other

- *(rpc,node)* [**breaking**] Fix clippy issues ([#626](https://github.com/eigerco/lumina/pull/626))

## [0.2.2](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.2.1...celestia-grpc-v0.2.2) - 2025-04-02

### Added

- lumina-utils crate ([#564](https://github.com/eigerco/lumina/pull/564))
- *(ci)* allow other node types than bridge ([#562](https://github.com/eigerco/lumina/pull/562))

## [0.2.1](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.2.0...celestia-grpc-v0.2.1) - 2025-02-24

### Other

- updated the following local packages: celestia-types

## [0.2.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.1.0...celestia-grpc-v0.2.0) - 2025-01-28

### Added

- *(grpc,node-wasm)* add javascript bindings for tx client (#510)
- *(grpc)* [**breaking**] add wasm support and transaction client (#474)

### Other

- *(ci)* migrate toolchain action, parallelize (#503)
- *(grpc)* Increase sleep before blob submission validation to reduce test flakyness (#481)

## [0.1.0](https://github.com/eigerco/lumina/releases/tag/celestia-grpc-v0.1.0) - 2024-12-02

### Added

- *(proto,types,rpc)* [**breaking**] celestia node v0.20.4 upgrade ([#469](https://github.com/eigerco/lumina/pull/469))
- *(grpc, types, proto)* [**breaking**] Add tonic gRPC ([#454](https://github.com/eigerco/lumina/pull/454))

### Other

- *(proto,types,node,grpc)* [**breaking**] Use `tendermint-rs` instead of `celestia-tendermint-rs` fork ([#463](https://github.com/eigerco/lumina/pull/463))
