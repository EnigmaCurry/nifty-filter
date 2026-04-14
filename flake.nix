{
  description = "nifty-filter - a nifty tool to configure netfilter/nftables";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in
    {
      packages = forAllSystems (system:
        let pkgs = pkgsFor system;
        in {
          nifty-filter = pkgs.rustPlatform.buildRustPackage {
            pname = "nifty-filter";
            version = "0.1.1";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            meta = {
              description = "A nifty tool to configure netfilter/nftables";
              license = pkgs.lib.licenses.mit;
              mainProgram = "nifty-filter";
            };
          };
          default = self.packages.${system}.nifty-filter;
        }
      );

      nixosModules.default = import ./nix/module.nix self;

      devShells = forAllSystems (system:
        let pkgs = pkgsFor system;
        in {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              cargo
              rustc
              rust-analyzer
              clippy
              rustfmt
              nftables
              just
            ];
          };
        }
      );
    };
}
