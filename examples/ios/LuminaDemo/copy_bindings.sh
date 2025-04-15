#!/usr/bin/env bash

set -euxo pipefail

cd "$(git rev-parse --show-toplevel)"

cp -rv node-uniffi/ios/lumina.xcframework examples/ios/LuminaDemo
cp -v node-uniffi/ios/*.swift examples/ios/LuminaDemo/LuminaDemo
