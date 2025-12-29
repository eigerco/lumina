# Lumina development justfile
# Run `just --list` to see all available commands

# Default recipe to display help
default:
    @just --list

# Install development dependencies
install-deps:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Installing development dependencies..."
    cargo install wasm-pack --version 0.13.1
    cargo install cargo-udeps
    cargo install cargo-msrv
    cargo install cargo-minimal-versions
    cargo install cargo-hack
    cargo install cargo-ndk --version 3.5.4
    rustup target add wasm32-unknown-unknown
    rustup target add aarch64-apple-ios aarch64-apple-ios-sim
    rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
    rustup component add clippy rustfmt

# Format all code
fmt:
    cargo fmt --all

# Check formatting without making changes
fmt-check:
    cargo fmt --all -- --check

# Run clippy for a specific crate on native target
clippy CRATE:
    cargo clippy --all-targets --all-features -p {{CRATE}} -- -D warnings --no-deps

# Run clippy for a specific crate on wasm32 target
clippy-wasm CRATE:
    cargo clippy --all-targets --all-features --target=wasm32-unknown-unknown -p {{CRATE}} -- -D warnings --no-deps

# Run clippy for all crates on all platforms
clippy-all: clippy-all-native clippy-all-wasm

# Run clippy for all crates on native target
clippy-all-native:
    #!/usr/bin/env bash
    set -euo pipefail
    crates=(
        "celestia-proto"
        "celestia-types"
        "celestia-rpc"
        "celestia-grpc"
        "celestia-grpc-macros"
        "celestia-client"
        "lumina-node"
        "lumina-node-uniffi"
        "lumina-node-wasm"
        "lumina-utils"
        "lumina-cli"
    )
    for crate in "${crates[@]}"; do
        echo "Running clippy for $crate (native)..."
        cargo clippy --all-targets --all-features -p "$crate" -- -D warnings --no-deps
    done

# Run clippy for all crates on wasm32 target
clippy-all-wasm:
    #!/usr/bin/env bash
    set -euo pipefail
    crates=(
        "celestia-proto"
        "celestia-types"
        "celestia-rpc"
        "celestia-grpc"
        "celestia-grpc-macros"
        "celestia-client"
        "lumina-node"
        "lumina-node-uniffi"
        "lumina-node-wasm"
        "lumina-utils"
        "lumina-cli"
    )
    for crate in "${crates[@]}"; do
        echo "Running clippy for $crate (wasm32)..."
        cargo clippy --all-targets --all-features --target=wasm32-unknown-unknown -p "$crate" -- -D warnings --no-deps
    done

# Run tests for a specific crate
test CRATE:
    cargo test --all-features -p {{CRATE}}

# Run all tests
test-all:
    cargo test --all-features

# Run wasm tests for a specific crate
test-wasm CRATE_DIR FLAGS="":
    wasm-pack test --headless --chrome {{CRATE_DIR}} {{FLAGS}}

# Build documentation
docs:
    cargo doc --workspace --all-features --no-deps

# Build documentation and open in browser
docs-open:
    cargo doc --workspace --all-features --no-deps --open

# Check documentation
docs-check:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Checking missing docs for native..."
    cargo clippy --workspace --all-features -- -D missing-docs
    echo "Checking missing docs for wasm32..."
    cargo clippy --workspace --all-features --target=wasm32-unknown-unknown -- -D missing-docs
    echo "Running rustdoc check..."
    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --no-deps

# Check for unused dependencies
check-unused-deps:
    cargo +nightly udeps --all-targets --all-features

# Check minimal versions of dependencies
check-minimal-versions:
    cargo minimal-versions check --direct --all-features
    cargo minimal-versions check --direct --all-features --target wasm32-unknown-unknown

# Verify MSRV for a specific crate
msrv CRATE_DIR:
    cargo msrv verify --manifest-path ./{{CRATE_DIR}}/Cargo.toml --all-features
    cargo msrv verify --manifest-path ./{{CRATE_DIR}}/Cargo.toml --all-features --target wasm32-unknown-unknown

# Run all checks (format, clippy, tests, docs)
check-all: fmt-check clippy-all test-all docs-check

# Build wasm node
build-wasm:
    wasm-pack build node-wasm

# Build and pack wasm node
build-wasm-pack:
    wasm-pack build node-wasm
    wasm-pack pack node-wasm

# Build demo webpage
build-demo-web:
    cd cli/js && npm ci && npm run build

# Build wasm with demo webpage
build-wasm-full: build-wasm build-demo-web

# Build iOS bindings
build-ios:
    ./node-uniffi/build-ios.sh
    ./examples/ios/LuminaDemo/copy_bindings.sh

# Build Android bindings
build-android:
    ./node-uniffi/build-android.sh
    ./examples/android/LuminaDemo/copy_bindings.sh

# Docker: Start Celestia devnet
docker-up:
    sudo docker compose -f ci/docker-compose.yml up --build --force-recreate -d

# Docker: Stop Celestia devnet
docker-down:
    docker compose -f ci/docker-compose.yml down

# Docker: View logs
docker-logs SERVICE="":
    #!/usr/bin/env bash
    if [ -z "{{SERVICE}}" ]; then
        docker compose -f ci/docker-compose.yml logs -f
    else
        docker compose -f ci/docker-compose.yml logs -f {{SERVICE}}
    fi

# Docker: Restart devnet
docker-restart: docker-down docker-up

# Docker: Get auth token for node-1
docker-auth-token:
    @docker compose -f ci/docker-compose.yml exec node-1 celestia bridge auth admin --p2p.network private

# Docker: Execute command in a service
docker-exec SERVICE CMD:
    docker compose -f ci/docker-compose.yml exec {{SERVICE}} {{CMD}}

# Docker: Get header by height
docker-get-header HEIGHT TOKEN:
    docker compose -f ci/docker-compose.yml exec node-1 celestia header get-by-height {{HEIGHT}} --token {{TOKEN}}

# Docker: Submit blob
docker-submit-blob NAMESPACE DATA TOKEN:
    docker compose -f ci/docker-compose.yml exec node-1 celestia blob submit {{NAMESPACE}} {{DATA}} --token {{TOKEN}}

# Docker: Clean up volumes
docker-clean:
    docker compose -f ci/docker-compose.yml down -v
    rm -rf ci/credentials/*

# Docker: View service status
docker-ps:
    docker compose -f ci/docker-compose.yml ps

# Generate authentication tokens for integration tests
gen-auth-tokens:
    ./tools/gen_auth_tokens.sh

# Run integration tests (requires devnet to be running)
test-integration:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Generating auth tokens..."
    ./tools/gen_auth_tokens.sh
    echo "Running integration tests..."
    cargo test --all-features

# Complete integration test workflow
test-integration-full: docker-restart gen-auth-tokens test-all docker-down

# Build CLI with browser support
build-cli-browser:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Building wasm node..."
    wasm-pack build node-wasm
    echo "Building demo webpage..."
    cd cli/js && npm ci && npm run build && cd -
    echo "Installing CLI with browser support..."
    cargo install --path cli --features browser-node

# Install CLI (basic)
install-cli:
    cargo install --path cli --locked

# Run lumina node on mocha network
run-mocha:
    cargo run --bin lumina-cli -- node --network mocha

# Run lumina node on mainnet
run-mainnet:
    cargo run --bin lumina-cli -- node --network mainnet

# Run lumina browser server
run-browser:
    cargo run --bin lumina-cli -- browser

# Clean build artifacts
clean:
    cargo clean
    rm -rf node-wasm/pkg
    rm -rf cli/js/node_modules
    rm -rf cli/js/dist

# Full clean including docker volumes
clean-all: clean docker-clean

# Run pre-commit checks
pre-commit: fmt clippy-all-native test-all

# Quick check (format + clippy native only)
quick-check: fmt clippy-all-native

# CI simulation - run all checks that CI runs
ci: fmt-check clippy-all test-all docs-check check-unused-deps check-minimal-versions
