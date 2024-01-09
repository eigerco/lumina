#!/bin/bash

set -xeuo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
cd ../proto

extract_urls() {
    local dir="$1"
    shift
    local urls=("$@")

    pushd "$dir" > /dev/null 2>&1

    for url in "${urls[@]}"; do
        curl -L "$url" | tar zxf -
    done

    popd > /dev/null 2>&1
}

rm -rf ../target/proto-vendor-src
mkdir -p ../target/proto-vendor-src

extract_urls ../target/proto-vendor-src \
    https://github.com/celestiaorg/celestia-app/archive/refs/tags/v1.4.0.tar.gz \
    https://github.com/celestiaorg/celestia-core/archive/refs/heads/v0.34.x-celestia.tar.gz \
    https://github.com/celestiaorg/celestia-node/archive/refs/heads/main.tar.gz \
    https://github.com/celestiaorg/cosmos-sdk/archive/refs/heads/release/v0.46.x-celestia.tar.gz \
    https://github.com/celestiaorg/nmt/archive/refs/heads/master.tar.gz \
    https://github.com/cosmos/cosmos-proto/archive/refs/tags/v1.0.0-alpha4.tar.gz \
    https://github.com/cosmos/gogoproto/archive/refs/tags/v1.4.11.tar.gz \
    https://github.com/celestiaorg/go-header/archive/refs/heads/main.tar.gz \
    https://github.com/googleapis/googleapis/archive/refs/heads/master.tar.gz

mkdir -p vendor

rm -rf vendor/celestia
cp -r ../target/proto-vendor-src/celestia-app-1.4.0/proto/celestia vendor

rm -rf vendor/go-header
mkdir -p vendor/go-header/p2p
cp -r ../target/proto-vendor-src/go-header-main/p2p/pb vendor/go-header/p2p

rm -rf vendor/cosmos
mkdir -p vendor/cosmos
cp -r ../target/proto-vendor-src/cosmos-sdk-release-v0.46.x-celestia/proto/cosmos/{base,staking,crypto,tx} vendor/cosmos

rm -rf vendor/cosmos_proto
cp -r ../target/proto-vendor-src/cosmos-proto-1.0.0-alpha4/proto/cosmos_proto vendor

rm -rf vendor/gogoproto
mkdir -p vendor/gogoproto
cp ../target/proto-vendor-src/gogoproto-1.4.11/gogoproto/gogo.proto vendor/gogoproto

rm -rf vendor/google
mkdir -p vendor/google/api
cp ../target/proto-vendor-src/googleapis-master/google/api/{annotations.proto,http.proto} vendor/google/api

rm -rf vendor/header
mkdir -p vendor/header
cp -r ../target/proto-vendor-src/celestia-node-main/header/pb vendor/header

rm -rf vendor/share
mkdir -p vendor/share
for pb_dir in ../target/proto-vendor-src/celestia-node-main/share/*/*/pb; do
    out_dir="${pb_dir#"${pb_dir%/*/*/*}"}"
    out_dir="vendor/share/${out_dir%/*}"
    mkdir -p "$out_dir"
    cp -r "$pb_dir" "$out_dir"
done

rm -rf vendor/tendermint
cp -r ../target/proto-vendor-src/celestia-core-0.34.x-celestia/proto/tendermint vendor

rm -rf vendor/nmt
mkdir -p vendor/nmt
cp -r ../target/proto-vendor-src/nmt-master/pb vendor/nmt

find vendor -name '*.go' -delete
