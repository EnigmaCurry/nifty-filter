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
          ./nix/platforms/filesystem.nix
        ];
      };

      version = self.shortRev or "dirty";
      sshKeys = builtins.getEnv "NIFTY_SSH_KEYS";

      # Build a PVE router system (disk-based, includes PVE-specific config)
      mkPveRouterSystem = { system, sshKeysArg ? sshKeys }:
        let
          nifty-filter-pkg = self.packages.${system}.nifty-filter;
        in
        nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = {
            inherit nifty-filter-pkg;
            sshKeys = sshKeysArg;
          };
          modules = [
            self.nixosModules.default
            ./nix/system.nix
            ./nix/platforms/pve-filesystem.nix
            ./nix/platforms/pve.nix
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
          specialArgs = { inherit version installedToplevel nifty-filter-pkg; };
          modules = [
            self.nixosModules.default
            ./nix/system.nix
            ./nix/platforms/iso.nix
          ] ++ extraModules;
        };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
        in {
          nifty-filter = pkgs.callPackage ./nix/packages/nifty-filter.nix { inherit version; };
          sodola-switch = pkgs.callPackage ./nix/packages/sodola-switch.nix {};
          nifty-dashboard = pkgs.callPackage ./nix/packages/nifty-dashboard.nix { inherit version; };
          nifty-service-monitor = pkgs.callPackage ./nix/packages/nifty-service-monitor.nix {};

          iso = (mkRouterIso { inherit system; }).config.system.build.isoImage;
          iso-big = (mkRouterIso { inherit system; extraModules = [ ./nix/platforms/iso-big.nix ]; }).config.system.build.isoImage;

          pve-image = import ./nix/platforms/pve-image.nix {
            inherit nixpkgs system self sshKeys version;
          };

          pve-image-ci = import ./nix/platforms/pve-image.nix {
            inherit nixpkgs system self version;
            sshKeys = "";
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
      nixosModules.services = import ./nix/services/containers/default.nix;
      nixosModules.step-ca = import ./nix/services/step-ca.nix;

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
