# Build a raw disk image for Proxmox VE deployment.
#
# Uses the native nixpkgs disk-image module (same approach as nixos-vm-template).
# Two-disk layout:
#   Disk 1 (boot+root): ESP + read-only NixOS system (built by disk-image module)
#                        Labels: ESP (boot), nixos (root)
#   Disk 2 (/var):       Created empty by pve-install, formatted on first boot
#
# Usage: NIFTY_SSH_KEYS="$(ssh-add -L)" nix build .#pve-image --impure
# Output: result/nifty-filter.qcow2
{ nixpkgs, system, self, sshKeys ? "", version ? "unknown" }:

let
  lib = nixpkgs.lib;
  nifty-filter-pkg = self.packages.${system}.nifty-filter;

  nixosConfig = nixpkgs.lib.nixosSystem {
    inherit system;
    specialArgs = {
      inherit nifty-filter-pkg sshKeys;
    };
    modules = [
      self.nixosModules.default
      ../system.nix
      # No filesystem.nix — PVE uses a two-disk layout managed by the disk-image module
      ./pve.nix
      # Native nixpkgs disk image module
      "${nixpkgs}/nixos/modules/virtualisation/disk-image.nix"
      {
        image.baseName = "nifty-filter";
        image.format = "qcow2";
        image.efiSupport = true;
        virtualisation.diskSize = 6 * 1024;  # 6 GiB (closure ~4G + headroom)

        # Disk 1: boot+root — use the labels the disk-image module creates
        # (it sets "/" and "/boot" automatically, we don't override them)

        # Disk 2: /var — pre-formatted by pve-install with label NIFTY_VAR
        fileSystems."/var" = {
          device = "/dev/disk/by-label/NIFTY_VAR";
          fsType = "ext4";
          options = [ "rw" "noatime" ];
          neededForBoot = true;
        };
        fileSystems."/home" = {
          device = "/var/home";
          fsType = "none";
          options = [ "bind" ];
          depends = [ "/var" ];
        };
        fileSystems."/root" = {
          device = "/var/root";
          fsType = "none";
          options = [ "bind" ];
          depends = [ "/var" ];
        };
        fileSystems."/tmp" = {
          device = "tmpfs";
          fsType = "tmpfs";
        };

        # Ensure directories exist on /var
        systemd.tmpfiles.rules = [
          "d /var/home 0755 root root -"
          "d /var/root 0700 root root -"
        ];

      }
    ];
  };
in
nixosConfig.config.system.build.image
