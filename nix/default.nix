_: {
  imports = [
    ./devshells.nix
    ./packages.nix
  ];
}
# { nixpkgs, ... }@inputs:
# let
#   systems = [
#     "x86_64-linux"
#     "aarch64-linux"
#     "aarch64-darwin"
#   ];
#   eachSystem =
#     fn:
#     nixpkgs.lib.genAttrs systems (
#       system:
#       fn (
#         inputs
#         // {
#           inherit system;
#           pkgs = nixpkgs."${system}".legacyPackages;
#         }
#       )
#     );
# in
# {
#   devShells = eachSystem (
# }
