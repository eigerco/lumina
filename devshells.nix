# `self'` is our flake's outputs but with `system` pre-selected
{ inputs', pkgs, ... }:

{
  devShells.default = pkgs.mkShell {
    packages = with pkgs; [
      pkg-config
      wasm-pack
    ];

    # inputsFrom = [ pkgs.swift ];
  };
}
