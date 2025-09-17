# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/eigerco/lumina/compare/celestia-client-v0.1.2...celestia-client-v0.2.0) - 2025-09-17

### Added

- *(grpc)* [**breaking**] Merge TxClient and GrpcClient, add builder ([#712](https://github.com/eigerco/lumina/pull/712))
- *(types)* [**breaking**] singular `Blob::new` constructor ([#719](https://github.com/eigerco/lumina/pull/719))

## [0.1.2](https://github.com/eigerco/lumina/compare/celestia-client-v0.1.1...celestia-client-v0.1.2) - 2025-09-08

### Added

- *(client,grpc)* make sure all returned futures are Send ([#729](https://github.com/eigerco/lumina/pull/729))

### Other

- *(client,grpc,types)* Make all the celestia-client types types Serialisable ([#734](https://github.com/eigerco/lumina/pull/734))

## [0.1.1](https://github.com/eigerco/lumina/compare/celestia-client-v0.1.0...celestia-client-v0.1.1) - 2025-08-19

### Other

- updated the following local packages: celestia-proto, celestia-types, celestia-types, celestia-rpc, celestia-rpc, celestia-grpc, celestia-grpc

## [0.1.0](https://github.com/eigerco/lumina/releases/tag/celestia-client-v0.1.0) - 2025-08-13

### Added

- *(proto,types,rpc)* [**breaking**] upgrade to celestia-node v0.25 ([#720](https://github.com/eigerco/lumina/pull/720))
- [**breaking**] Implement `celestia-client` crate ([#682](https://github.com/eigerco/lumina/pull/682))
