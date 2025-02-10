# `self'` is our flake's outputs but with `system` pre-selected
{ self', pkgs, ... }:

{
  devShells.default = pkgs.mkShell.override { inherit (pkgs.llvm) stdenv; } {
    packages = with pkgs; [
      self'.packages.swift
      pkg-config
      wasm-pack
    ];

    # inputsFrom = [ pkgs.swift ];
  };
}
