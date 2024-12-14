#!/bin/bash
 
cargo build -p native

mkdir -p ./bindings
mkdir -p ./ios
 
cargo run --bin uniffi-bindgen generate --library ../target/debug/libnative.dylib --language swift --out-dir ./bindings
 
for TARGET in \
        aarch64-apple-darwin \
        aarch64-apple-ios \
        aarch64-apple-ios-sim \
        x86_64-apple-darwin \
        x86_64-apple-ios
do
    rustup target add $TARGET
    cargo build --release --target=$TARGET
done
 
mv ./bindings/nativeFFI.modulemap ./bindings/module.modulemap
 
rm ./ios/Native.swift
mv ./bindings/native.swift ./ios/Native.swift
 
rm -rf "ios/Native.xcframework"
xcodebuild -create-xcframework \
        -library ../target/aarch64-apple-ios-sim/release/libnative.a -headers ./bindings \
        -library ../target/aarch64-apple-ios/release/libnative.a -headers ./bindings \
        -output "ios/Native.xcframework"
 
rm -rf bindings