{
  description = "Approval-sealed manifests for external communications";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];

      imports = [ inputs.treefmt-nix.flakeModule ];

      perSystem =
        { pkgs, ... }:
        {
          packages.default = pkgs.rustPlatform.buildRustPackage {
            pname = "outbound-manifest";
            version = "0.1.0";
            src = pkgs.lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;
            meta = {
              description = "Review, seal, and verify external communications";
              homepage = "https://github.com/conao3/rust-outbound-manifest";
              license = pkgs.lib.licenses.mit;
              mainProgram = "outbound-manifest";
              platforms = pkgs.lib.platforms.unix;
            };
          };

          devShells.default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clippy
              rustc
              rustfmt
            ];
          };

          treefmt.programs = {
            nixfmt.enable = true;
            rustfmt.enable = true;
          };
        };
    };
}
