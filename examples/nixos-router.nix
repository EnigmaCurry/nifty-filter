# Example: using the nifty-filter NixOS module in your own system
#
# The module provides an read-only-friendly service that reads
# /var/nifty-filter/router.env at boot and applies nftables rules.
#
# In your flake.nix:
#
#   inputs.nifty-filter.url = "github:EnigmaCurry/nifty-filter/nixos";
#
#   nixosConfigurations.my-router = nixpkgs.lib.nixosSystem {
#     modules = [
#       nifty-filter.nixosModules.default
#       ./configuration.nix
#     ];
#   };
#
# Then place your router.env on the /var partition:
#
#   /var/nifty-filter/router.env
#
# A default env file is seeded on first boot if none exists.
# Edit it and reboot (or `systemctl restart nifty-filter`) to apply.
{ config, pkgs, ... }:

{
  services.nifty-filter.enable = true;

  # Optional: override the config path
  # services.nifty-filter.configPath = "/var/my-custom/firewall.env";
}
