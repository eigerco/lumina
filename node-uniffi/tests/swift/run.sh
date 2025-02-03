#!/bin/sh
set -xeuo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

cd ../../
cargo build

rm -rf ../target/swift-uniffi-bindings
mkdir -p ../target/swift-uniffi-bindings

cargo run --bin uniffi-bindgen generate --library ../target/debug/liblumina_node_uniffi.a --language swift --out-dir ../target/swift-uniffi-bindings

cd tests/swift

rm -rf lib
rm -rf Sources/LuminaNodeHeaders
rm -rf Sources/LuminaNode

mkdir lib
mkdir Sources/LuminaNodeHeaders
mkdir Sources/LuminaNode

cp ../../../target/debug/liblumina_node_uniffi.a lib
cp ../../../target/swift-uniffi-bindings/*.swift Sources/LuminaNode
cp ../../../target/swift-uniffi-bindings/*.h Sources/LuminaNodeHeaders
cat ../../../target/swift-uniffi-bindings/*.modulemap > Sources/LuminaNodeHeaders/module.modulemap

swift test
