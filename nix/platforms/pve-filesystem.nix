# Filesystem layout for PVE disk-image installs.
#
# Must match what the disk-image module creates in pve-image.nix:
#   - Root: ext4, label "nixos" (disk-image module default)
#   - Boot: vfat ESP, label "ESP" (disk-image module default)
#   - /var: ext4, label "NIFTY_VAR" (created by pve-install)
# Mutable paths (/home, /root) bind-mount from /var.
{ lib, ... }:

{
  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/ESP";
    fsType = "vfat";
  };
  fileSystems."/var" = {
    device = "/dev/disk/by-label/NIFTY_VAR";
    fsType = "ext4";
    options = [ "rw" "noatime" ];
    neededForBoot = true;
  };

  fileSystems."/home" = { device = "/var/home"; fsType = "none"; options = [ "bind" ]; depends = [ "/var" ]; };
  fileSystems."/root" = { device = "/var/root"; fsType = "none"; options = [ "bind" ]; depends = [ "/var" ]; };
  fileSystems."/tmp" = { device = "tmpfs"; fsType = "tmpfs"; };

  systemd.tmpfiles.rules = [
    "d /var/home 0755 root root -"
    "d /var/root 0700 root root -"
  ];
}
