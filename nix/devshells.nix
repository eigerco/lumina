{ pkgs, lib, ... }:
let
  extraTargets =
    [
      # wasm
      "wasm32-unknown-unknown"
      # android
      "aarch64-linux-android"
      "armv7-linux-androideabi"
      "x86_64-linux-android"
      "i686-linux-android"
    ]
    # ios
    ++ lib.optionals pkgs.stdenv.isDarwin [
      "aarch64-apple-ios"
      "aarch64-apple-ios-sim"
    ];

  rustToolchain =
    with pkgs;
    fenix.combine [
      # components
      (fenix.stable.withComponents [
        "cargo"
        "clippy"
        "rustc"
        "rustfmt"
        "rust-analyzer"
      ])
      # extra targets
      (lib.lists.map (target: fenix.targets."${target}".stable.rust-std) extraTargets)
    ];

  # rustup-like-ish wrapper for cargo to allow `+nightly` syntax
  cargoWrapper =
    with pkgs;
    writeShellScriptBin "cargo" ''
      if [ "$1" == "+nightly" ]; then
          shift
          exec env PATH="${fenix.minimal.toolchain}/bin:$PATH" cargo "$@"
      fi
      exec ${rustToolchain}/bin/cargo "$@"
    '';
in
{
  devShells.default = pkgs.mkShell {
    packages = with pkgs; [
      # c/gnu base
      gnumake
      pkg-config
      stdenv

      # rust - toolchain
      (lib.hiPrio cargoWrapper) # should be first in PATH
      rustToolchain
      # rust - wasm
      binaryen # wasm-opt
      wasm-bindgen-cli_0_2_100
      wasm-pack
      # rust - other
      cargo-edit
      cargo-ndk
      cargo-udeps

      # javascript
      nodejs
      nodePackages.typescript-language-server

      # go
      gopls
    ];
  };
}
