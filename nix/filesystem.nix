# Immutable filesystem layout for installed systems (not the ISO)
#
# Root is read-only. /var is the only writable partition.
# Mutable paths (/home, /root) are bind-mounted from /var.
# Partitions are identified by label, not device path.
{ lib, ... }:

{
  fileSystems."/" = {
    device = "/dev/disk/by-label/NIFTY_ROOT";
    fsType = "ext4";
    options = [ "ro" ];
  };
  fileSystems."/boot" = {
    device = "/dev/disk/by-label/NIFTY_BOOT";
    fsType = "vfat";
  };
  fileSystems."/var" = {
    device = "/dev/disk/by-label/NIFTY_VAR";
    fsType = "ext4";
    options = [ "rw" "noatime" ];
    neededForBoot = true;
  };

  fileSystems."/home" = { device = "/var/home"; options = [ "bind" ]; };
  fileSystems."/root" = { device = "/var/root"; options = [ "bind" ]; };
  fileSystems."/tmp" = { device = "tmpfs"; fsType = "tmpfs"; };
}
