#!/usr/bin/env bash
set -xeuo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

target_dir="$(realpath ../../../target)"

rm -rf "$target_dir"/swift-uniffi-bindings
mkdir -p "$target_dir"/swift-uniffi-bindings

cargo build --lib --bin uniffi-bindgen

"$target_dir"/debug/uniffi-bindgen \
  generate \
  --library ../target/debug/liblumina_node_uniffi.a \
  --language swift --out-dir \
  ../target/swift-uniffi-bindings

cd tests/swift

rm -rf lib
rm -rf Sources/LuminaNodeHeaders
rm -rf Sources/LuminaNode

mkdir lib
mkdir -p Sources/LuminaNodeHeaders
mkdir -p Sources/LuminaNode

cp "$target_dir"/debug/liblumina_node_uniffi.a lib
cp "$target_dir"/swift-uniffi-bindings/*.swift Sources/LuminaNode
cp "$target_dir"/swift-uniffi-bindings/*.h Sources/LuminaNodeHeaders
cat "$target_dir"/swift-uniffi-bindings/*.modulemap > Sources/LuminaNodeHeaders/module.modulemap

if [ -n "${CI:-}" ]; then
  # On CI there is an issue with permissions as runner has non-standard user & group ids
  user=0:0
else
  user="$(id -u):$(id -g)"
fi

# if user doesn't belong to docker group, run docker with sudo
SUDO=""
if ! id -nG | grep -q docker; then
  SUDO=sudo
fi

$SUDO docker run \
  --rm \
  --user "$user" \
  --volume "$PWD":/app \
  --workdir /app \
  swift:6 \
  swift test
