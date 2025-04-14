#!/usr/bin/env bash

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
    https://github.com/celestiaorg/celestia-app/archive/refs/tags/v3.0.2.tar.gz \
    https://github.com/celestiaorg/celestia-node/archive/refs/heads/main.tar.gz \
    https://github.com/celestiaorg/cosmos-sdk/archive/refs/heads/release/v0.46.x-celestia.tar.gz \
    https://codeload.github.com/cometbft/cometbft/tar.gz/refs/tags/v0.34.35 \
    https://github.com/celestiaorg/nmt/archive/refs/heads/main.tar.gz \
    https://github.com/cosmos/cosmos-proto/archive/refs/tags/v1.0.0-alpha7.tar.gz \
    https://github.com/cosmos/gogoproto/archive/refs/tags/v1.4.11.tar.gz \
    https://github.com/celestiaorg/go-header/archive/refs/heads/main.tar.gz \
    https://github.com/celestiaorg/go-square/archive/refs/heads/main.tar.gz \
    https://github.com/googleapis/googleapis/archive/refs/heads/master.tar.gz \

mkdir -p vendor

rm -rf vendor/celestia
cp -r ../target/proto-vendor-src/celestia-app-3.0.2/proto/celestia vendor

rm -rf vendor/go-header
mkdir -p vendor/go-header/p2p
cp -r ../target/proto-vendor-src/go-header-main/p2p/pb vendor/go-header/p2p

rm -rf vendor/go-square
cp -r ../target/proto-vendor-src/go-square-main/proto vendor/go-square

rm -rf vendor/cosmos
mkdir -p vendor/cosmos
cp -r ../target/proto-vendor-src/cosmos-sdk-release-v0.46.x-celestia/proto/cosmos/{auth,bank,base,msg,staking,crypto,tx} vendor/cosmos

rm -rf vendor/cosmos_proto
cp -r ../target/proto-vendor-src/cosmos-proto-1.0.0-alpha7/proto/cosmos_proto vendor

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
shwap_dir=../target/proto-vendor-src/celestia-node-main/share
find "$shwap_dir" -name pb -type d -print0 | while read -r -d '' pb_dir; do
  # remove prefix
  out_dir="${pb_dir#"$shwap_dir"}"
  # remove /pb suffix
  out_dir="vendor/share/${out_dir%/*}"
  mkdir -p "$out_dir"
  cp -r "$pb_dir" "$out_dir"
done

rm -rf vendor/tendermint
cp -r ../target/proto-vendor-src/cometbft-0.34.35/proto/tendermint vendor

rm -rf vendor/nmt
mkdir -p vendor/nmt
cp -r ../target/proto-vendor-src/nmt-main/pb vendor/nmt

find vendor -name '*.go' -delete
