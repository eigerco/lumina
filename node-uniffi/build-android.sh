#!/bin/bash

cargo build -p lumina-node-uniffi

rustup target add \
    aarch64-linux-android \
    armv7-linux-androideabi \
    x86_64-linux-android \
    i686-linux-android

cargo ndk -o ./app/src/main/jniLibs \
        --manifest-path ./Cargo.toml \
        -t armeabi-v7a \
        -t arm64-v8a \
        -t x86 \
        -t x86_64 \
        build --release

cargo run --bin uniffi-bindgen generate --library ../target/debug/liblumina_node_uniffi.dylib --language kotlin --out-dir ./app/src/main/java/tech/forgen/lumina_node_uniffi/rust

echo "Android build complete"
