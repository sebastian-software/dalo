{
  description = "Dalo, a Git-backed skill manager for AI agents";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          dalo = pkgs.callPackage ./nix/package.nix { };
        in
        {
          inherit dalo;
          default = dalo;
        }
      );
    };
}
