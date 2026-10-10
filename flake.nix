{
  description = "ApexShot: Linux screenshot, annotation, and screen recording tool";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forSystems (system: rec {
        apexshot = nixpkgs.legacyPackages.${system}.callPackage ./packaging/nix/package.nix { };
        default = apexshot;
      });

      apps = forSystems (system: rec {
        apexshot = {
          type = "app";
          program = "${self.packages.${system}.apexshot}/bin/apexshot";
        };
        default = apexshot;
      });
    };
}
