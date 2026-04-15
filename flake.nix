{
  description = "nifty-filter - a nifty tool to configure netfilter/nftables";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    script-wizard.url = "github:EnigmaCurry/script-wizard/master";
  };

  outputs = { self, nixpkgs, script-wizard }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};

      # Build a NixOS system with nifty-filter for a given architecture
      mkRouterSystem = system: nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = {
          scriptWizard = script-wizard.packages.${system}.default;
        };
        modules = [
          self.nixosModules.default
          ./nix/system.nix
          ./nix/filesystem.nix
        ];
      };

      version = self.shortRev or "dirty";

      # Build an ISO image for a given architecture.
      # The ISO embeds the installed system closure so the installer
      # can copy it to disk (the ISO's own system boots from squashfs
      # and can't be used as a disk-based system).
      mkRouterIso = system:
        let
          installedSystem = mkRouterSystem system;
          installedToplevel = installedSystem.config.system.build.toplevel;
          scriptWizard = script-wizard.packages.${system}.default;
        in
        nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = { inherit version installedToplevel scriptWizard; };
          modules = [
            self.nixosModules.default
            ./nix/system.nix
            ./nix/iso.nix
          ];
        };
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

          iso = (mkRouterIso system).config.system.build.isoImage;

          default = self.packages.${system}.nifty-filter;
        }
      );

      nixosConfigurations = {
        router-x86_64 = mkRouterSystem "x86_64-linux";
        router-aarch64 = mkRouterSystem "aarch64-linux";
      };

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
