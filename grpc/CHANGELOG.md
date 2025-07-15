# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/eigerco/lumina/compare/celestia-grpc-v0.4.1...celestia-grpc-v0.5.0) - 2025-07-15

### Added

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
