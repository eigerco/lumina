#!/usr/bin/env bash

set -euxo pipefail

cd "$(git rev-parse --show-toplevel)"

cp -rv ~/dev/eiger/uniffi-grpc-client/node-uniffi/app examples/android/LuminaDemo
