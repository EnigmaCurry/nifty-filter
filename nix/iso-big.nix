# "Big" ISO variant with full hardware support (linux-firmware + all drivers).
# Build with: nix build .#iso-big
{ lib, modulesPath, ... }:

{
  imports = [
    "${modulesPath}/profiles/all-hardware.nix"
  ];

  hardware.enableRedistributableFirmware = lib.mkForce true;
}
