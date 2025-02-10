{ pkgs, lib, ... }:

let
  ver = "6.0.3";
  os = if pkgs.stdenv.isAarch64 then "debian12-aarch64" else "debian12";
  swift_src = {
    url = "https://download.swift.org/swift-${ver}-release/${os}/swift-${ver}-RELEASE/swift-${ver}-RELEASE-${os}.tar.gz";
    sha256 =
      if pkgs.stdenv.isAarch64 then
        "sha256-CRjUSVZdDR5dZWQtfg0n0XOuzeG0IO/YQFOmybTrGLI="
      else
        "sha256-b3ggPksAN8js+uiQlkM+sO8ZjSCuSy6/rfPAQgnSzeI=";
  };
in
{
  packages.swift = pkgs.stdenv.mkDerivation {
    pname = "swift";
    version = "6.0.3";

    src = pkgs.fetchurl swift_src;

    nativeBuildInputs = [
      pkgs.autoPatchelfHook
      pkgs.makeWrapper
    ];

    propagatedBuildInputs = with pkgs; [
      # gcc stack
      llvm.stdenv.cc.cc.lib
      # others
      curl
      sqlite
      python311
      libedit
      libuuid
      libxml2
    ];

    installPhase = # sh
      ''
        mkdir -p "$out/opt/swift"
        cp -ra * "$out/opt/swift"

        # hack, libedit is shipped as .so.0 but swift wants .so.2
        ln -s "${pkgs.libedit}/lib/libedit.so.0" "$out/opt/swift/usr/lib/swift/linux/libedit.so.2"

        mkdir -p "$out/bin"

        for x in "$out/opt/swift/usr/bin"/*swift*; do
          if [[ -x "$x" ]]; then
            dest="$out/bin/''${x##*/}"
            makeWrapper "$x" "$dest" --prefix PATH : "$out/opt/swift/usr/bin"
          fi
        done
      '';
  };
}
