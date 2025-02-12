{ pkgs, ... }:

{
  packages = [
    pkgs.protobuf
    pkgs.wasm-pack
  ];

  cachix.enable = false;
  dotenv.enable = true;

  languages.rust = {
    enable = true;
    channel = "stable";
    # Extra targets other than the native
    targets = [ "wasm32-unknown-unknown" ];
  };

  languages.javascript = {
    enable = true;
    npm.enable = true;
  };

  languages.kotlin.enable = true;
}
