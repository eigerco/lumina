# Celestia proto

This crate aggregates and generates the rust types from the google protobuf `.proto` files used in the Celestia network.
The types are meant to support also `serde` serialization and deserialization in the same
format that is understood by the [`celestia-node`](https://github.com/celestiaorg/celestia-node).

For more details on what exactly is vendored and how to update the protos, see the [`vendor`](./vendor/README.md).
