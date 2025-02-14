{
  inputs',
  pkgs,
  lib,
  ...
}:

let
  fenix = inputs'.fenix.packages;

  # creates list of components taken from a toolchain for given channel
  rustComponents =
    channel:
    [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
    ]
    ++ lib.optionals (channel == "stable") [
      "rust-analyzer"
    ];

  # creates list of the extra targets to be installed for given channel
  rustExtraTargets =
    channel:
    [
      "wasm32-unknown-unknown"
    ]
    # android
    ++ lib.optionals (channel == "stable") [
      "aarch64-linux-android"
      "armv7-linux-androideabi"
      "x86_64-linux-android"
      "i686-linux-android"
    ]
    # ios
    ++ lib.optionals (pkgs.stdenv.isDarwin && channel == "stable") [
      "aarch64-apple-ios"
      "aarch64-apple-ios-sim"
    ];

  # creates toolchain configuration for given channel
  rustToolchain =
    channel:
    fenix.combine [
      # components
      (fenix."${channel}".withComponents (rustComponents channel))
      # extra targets
      (lib.lists.map (target: fenix.targets."${target}"."${channel}".rust-std) (rustExtraTargets channel))
    ];

  # rustup-like-ish wrapper for cargo to allow `+channel` syntax
  cargoWrapper = pkgs.writeShellScriptBin "cargo" ''
    case "$1" in
      "+stable")
        shift
        exec env PATH="${rustToolchain "stable"}/bin:$PATH" cargo "$@"
        ;;
      "+nightly")
        shift
        exec env PATH="${rustToolchain "latest"}/bin:$PATH" cargo "$@"
        ;;
      "+"*)
        echo "Channel $1 not installed. You may need to add it to the devShell" >&2
        exit 1
        ;;
      *)
        exec env PATH="${rustToolchain "stable"}/bin:$PATH" cargo "$@"
        ;;
    esac
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
      cargoWrapper # first in PATH, to be invoked instead one from toolchain
      (rustToolchain "stable")
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
    ];
  };
}
