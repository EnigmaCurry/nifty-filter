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

      # Build a NixOS system with nifty-filter for a given architecture
      mkRouterSystem = system: nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = {};
        modules = [
          self.nixosModules.default
          ./nix/system.nix
          ./nix/filesystem.nix
        ];
      };

      version = self.shortRev or "dirty";
      gitBranch = builtins.getEnv "NIFTY_BUILD_BRANCH";
      sshKeys = builtins.getEnv "NIFTY_SSH_KEYS";

      # Build a PVE router system (disk-based, includes PVE-specific config)
      mkPveRouterSystem = { system, sshKeysArg ? sshKeys }:
        let
          nifty-filter-pkg = self.packages.${system}.nifty-filter;
        in
        nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = {
            inherit gitBranch nifty-filter-pkg;
            sshKeys = sshKeysArg;
          };
          modules = [
            self.nixosModules.default
            ./nix/system.nix
            ./nix/pve-filesystem.nix
            ./nix/pve.nix
          ];
        };

      # Build an ISO image for a given architecture.
      # The ISO embeds the installed system closure so the installer
      # can copy it to disk (the ISO's own system boots from squashfs
      # and can't be used as a disk-based system).
      mkRouterIso = { system, extraModules ? [] }:
        let
          installedSystem = mkRouterSystem system;
          installedToplevel = installedSystem.config.system.build.toplevel;
          nifty-filter-pkg = self.packages.${system}.nifty-filter;
        in
        nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = { inherit version installedToplevel gitBranch nifty-filter-pkg; };
          modules = [
            self.nixosModules.default
            ./nix/system.nix
            ./nix/iso.nix
          ] ++ extraModules;
        };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          lib = pkgs.lib;
        in {
          nifty-filter = pkgs.rustPlatform.buildRustPackage {
            pname = "nifty-filter";
            version = "0.2.1";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            buildFeatures = [ "nixos" ];
            cargoBuildFlags = [ "-p" "nifty-filter" ];
            GIT_SHA = version;
            meta = {
              description = "A nifty tool to configure netfilter/nftables";
              license = pkgs.lib.licenses.mit;
              mainProgram = "nifty-filter";
            };
          };

          sodola-switch = pkgs.rustPlatform.buildRustPackage {
            pname = "sodola-switch";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" "sodola-switch" ];
            meta = {
              description = "Management client for Sodola SL-SWTGW218AS managed switch";
              license = pkgs.lib.licenses.mit;
              mainProgram = "sodola-switch";
            };
          };

          nifty-dashboard =
            let
              frontend = pkgs.stdenv.mkDerivation {
                pname = "nifty-dashboard-frontend";
                version = "0.1.0";
                src = ./crates/nifty-dashboard/frontend;
                nativeBuildInputs = [
                  pkgs.pnpm
                  pkgs.pnpmConfigHook
                  pkgs.nodejs
                ];
                pnpmDeps = pkgs.fetchPnpmDeps {
                  pname = "nifty-dashboard-frontend";
                  version = "0.1.0";
                  src = ./crates/nifty-dashboard/frontend;
                  hash = "sha256-PCIjOq4qHY/I/TvU+pdOBbWWdhETwsuxwaehbVm1hg8=";
                  fetcherVersion = 2;
                };
                buildPhase = ''
                  pnpm build
                '';
                installPhase = ''
                  cp -r build $out
                '';
              };
            in
            pkgs.rustPlatform.buildRustPackage {
              pname = "nifty-dashboard";
              version = "0.1.0";
              src = ./crates/nifty-dashboard;
              cargoLock = {
                lockFile = ./crates/nifty-dashboard/Cargo.lock;
                outputHashes = {
                  "conf-0.4.5" = "sha256-gxxB8t0bl8ZudylXe4edAIVjO4KNHZshUhifvpm1b5E=";
                };
              };
              cargoBuildFlags = [ "-p" "nifty-dashboard" ];
              GIT_SHA = version;
              nativeBuildInputs = [ pkgs.pkg-config ];
              buildInputs = [ pkgs.openssl ];
              preBuild = ''
                rm -rf frontend/build
                ln -s ${frontend} frontend/build
                cp ${./LICENSE.md} LICENSE.md
              '';
              meta = {
                description = "Web dashboard for nifty-filter";
                license = pkgs.lib.licenses.mit;
                mainProgram = "nifty-dashboard";
              };
            };

          iso = (mkRouterIso { inherit system; }).config.system.build.isoImage;
          iso-big = (mkRouterIso { inherit system; extraModules = [ ./nix/iso-big.nix ]; }).config.system.build.isoImage;

          pve-image = import ./nix/pve-image.nix {
            inherit nixpkgs system self sshKeys version gitBranch;
          };

          default = self.packages.${system}.nifty-filter;
        }
      );

      nixosConfigurations = {
        router-x86_64 = mkRouterSystem "x86_64-linux";
        router-aarch64 = mkRouterSystem "aarch64-linux";
        pve-router-x86_64 = mkPveRouterSystem { system = "x86_64-linux"; };
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
              pkg-config
              openssl
            ];
          };
        }
      );
    };
}
