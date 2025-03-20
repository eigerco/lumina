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

  # ios support heavily based on:
  # https://github.com/fedibtc/flakebox/blob/master/lib/mkIOSTarget.nix
  # thanks :pray:

  ios-targets = [
    "aarch64-apple-ios"
    "aarch64-apple-ios-sim"
  ];

  # make bindgen (clang-sys) crate use /usr/bin/clang instead of NixOS
  ios-clang-wrapper =
    target:
    pkgs.writeShellScriptBin "${target}-clang" ''
      exec /usr/bin/clang "$@"
    '';

  # older bindgen (clang-sys) crate can be told to use /usr/bin/clang this way
  ios-llvm-config-wrapper = pkgs.writeShellScriptBin "llvm-config" ''
    if [ "$1" == "--bindir" ]; then
      echo "/usr/bin/"
      exit 0
    fi
    exec llvm-config "$@"
  '';

  ios-shell-hook = lib.strings.concatMapStringsSep "\n" (
    target:
    let
      target_underscores = lib.strings.replaceStrings [ "-" ] [ "_" ] target;
      target_underscores_upper = lib.strings.toUpper target_underscores;
    in
    # bash
    ''
      # For older bindgen, through universal-llvm-config
      export LLVM_CONFIG_PATH_${target_underscores}=${ios-llvm-config-wrapper}/bin/llvm-config;

      export CC_${target_underscores}=/usr/bin/clang;
      export CXX_${target_underscores}=/usr/bin/clang++;
      export LD_${target_underscores}=/usr/bin/cc;

      export CARGO_TARGET_${target_underscores_upper}_LINKER=/usr/bin/clang;
      export CARGO_TARGET_${target_underscores_upper}_RUSTFLAGS="-C link-arg=-fuse-ld=/usr/bin/ld";
    ''
  ) ios-targets;
in
{
  devShells.default = pkgs.mkShell {
    packages =
      with pkgs;
      [
        # c/gnu base
        gnumake
        pkg-config
        stdenv

        # rust - toolchain
        (lib.hiPrio cargoWrapper) # should be first in PATH
        rustToolchain
        # rust - wasm
        binaryen # wasm-opt
        chromedriver
        geckodriver
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
      ]
      ++ lib.optionals stdenv.isDarwin [
        # swift lsp
        sourcekit-lsp
        # clang
        (lib.lists.map (target: ios-clang-wrapper target) ios-targets)
        # xcode
        (xcodeenv.composeXcodeWrapper { versions = [ "16.2" ]; })
      ];

    shellHook = lib.optionalString pkgs.stdenv.isDarwin (
      ''
        export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
      ''
      + ios-shell-hook
    );

    WASM_BINDGEN_TEST_TIMEOUT = 120;
  };
}
