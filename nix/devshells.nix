# `self'` is our flake's outputs but with `system` pre-selected
{ inputs', pkgs, ... }:

{
  devShells.default = pkgs.mkShell {
    packages = with pkgs; [
      # inputs'.swift-bin.packages.swift
      pkgs.llvmPackages_17.clang
      pkg-config
      wasm-pack
    ];

    # inputsFrom = [ pkgs.swift ];
  };
}
