# Immutable filesystem layout for installed systems (not the ISO)
#
# Root is read-only. /var is the only writable partition.
# Mutable paths (/home, /root) are bind-mounted from /var.
{ lib, ... }:

{
  fileSystems."/" = {
    device = lib.mkDefault "/dev/vda2";
    fsType = "ext4";
    options = [ "ro" ];
  };
  fileSystems."/boot" = {
    device = lib.mkDefault "/dev/vda1";
    fsType = "vfat";
  };
  fileSystems."/var" = {
    device = lib.mkDefault "/dev/vdb1";
    fsType = "ext4";
    options = [ "rw" "noatime" ];
    neededForBoot = true;
  };

  fileSystems."/home" = { device = "/var/home"; options = [ "bind" ]; };
  fileSystems."/root" = { device = "/var/root"; options = [ "bind" ]; };
  fileSystems."/tmp" = { device = "tmpfs"; fsType = "tmpfs"; };
}
