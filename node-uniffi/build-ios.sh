#!/usr/bin/env bash

set -euxo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

# create swift bindings

rm -rf ./bindings ./ios
mkdir -p ./bindings
mkdir -p ./ios
mkdir -p ./bindings/Headers

cargo build
cargo run --bin uniffi-bindgen \
  generate \
  --library ../target/debug/liblumina_node_uniffi.dylib \
  --language swift \
  --out-dir ./bindings

cat ./bindings/lumina_node_uniffiFFI.modulemap ./bindings/lumina_nodeFFI.modulemap > ./bindings/Headers/module.modulemap

cp ./bindings/*.h ./bindings/Headers/

rm -rf ./ios/lumina.xcframework

# create xcode project

for target in aarch64-apple-ios aarch64-apple-ios-sim; do
  cargo build --lib --release --target="$target"
done

xcodebuild -create-xcframework \
        -library ../target/aarch64-apple-ios-sim/release/liblumina_node_uniffi.a -headers ./bindings/Headers \
        -library ../target/aarch64-apple-ios/release/liblumina_node_uniffi.a -headers ./bindings/Headers \
        -output "ios/lumina.xcframework"

cp ./bindings/*.swift ./ios/

rm -rf bindings
