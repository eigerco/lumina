#!/usr/bin/env bash

set -euxo pipefail

cd "$(git rev-parse --show-toplevel)"

cp -rv node-uniffi/app examples/android
