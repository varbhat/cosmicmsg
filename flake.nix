{
  description = "cosmicmsg - swaymsg-equivalent CLI for the COSMIC desktop";

  inputs = {
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs: let
    inherit (inputs.nixpkgs) lib;
  in
    inputs.flake-parts.lib.mkFlake {inherit inputs;} {
      systems = [
        "aarch64-linux"
        "x86_64-linux"
      ];

      imports = lib.optionals (inputs.treefmt-nix ? flakeModule) [
        inputs.treefmt-nix.flakeModule
      ];

      perSystem = {
        pkgs,
        self',
        ...
      }:
        {
          devShells.default = import ./shell.nix {inherit pkgs;};

          packages = {
            default = self'.packages.cosmicmsg;
            cosmicmsg = pkgs.rustPlatform.buildRustPackage {
              pname = "cosmicmsg";
              version = "0.1.0";
              src = ./.;
              cargoLock = {
                lockFile = ./Cargo.lock;
              };
              nativeBuildInputs = with pkgs; [pkg-config];
              buildInputs = with pkgs; [
                libxkbcommon
                wayland
              ];
              meta = {
                description = "CLI tool for querying and controlling the COSMIC desktop";
                mainProgram = "cosmicmsg";
              };
            };
          };
        }
        // lib.optionalAttrs (inputs.treefmt-nix ? flakeModule) {
          treefmt.config = {
            projectRootFile = "flake.nix";

            programs = {
              alejandra.enable = true;
              rustfmt.enable = true;
            };
          };
        };
    };
}
