{
  description = "Melaffeine";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (
        system:
        import ./nix/flake/packages.nix {
          pkgs = pkgsFor system;
          src = self;
        }
      );

      devShells = forAllSystems (
        system:
        import ./nix/flake/devShells.nix {
          pkgs = pkgsFor system;
        }
      );

      formatter = forAllSystems (
        system:
        import ./nix/flake/formatter.nix {
          pkgs = pkgsFor system;
        }
      );
    };
}
