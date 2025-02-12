{ inputs', pkgs, ... }:

let
  fenix = inputs'.fenix.packages;
  rust-toolchain =
    channel:
    fenix.combine [
      (fenix."${channel}".withComponents [
        "rustc"
        "cargo"
        "clippy"
        "rustfmt"
        "rust-analyzer"
      ])
      fenix.targets.wasm32-unknown-unknown.stable.rust-std
    ];
in
{
  devShells.default = pkgs.mkShell {
    packages = with pkgs; [
      # c
      gnumake
      stdenv
      pkg-config

      # rust
      binaryen # wasm-opt
      cargo-udeps
      (pkgs.writeShellScriptBin "cargo" ''
        case "$1" in
          "+nightly")
            shift
            exec ${lib.getExe' (rust-toolchain "latest") "cargo"} "$@"
            ;;
          "+stable")
            shift
            ;&
          *)
            exec ${lib.getExe' (rust-toolchain "stable") "cargo"} "$@"
            ;;
        esac
      '')
      (rust-toolchain "stable")
      wasm-bindgen-cli_0_2_100
      wasm-pack
    ];
  };
}
