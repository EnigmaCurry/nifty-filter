# Main nifty-filter CLI tool.
# Generates nftables rulesets, networkd configs, and dnsmasq configs from HCL.
{ lib, rustPlatform, version ? "unknown" }:

rustPlatform.buildRustPackage {
  pname = "nifty-filter";
  version = "0.2.1";
  src = ../../.;
  cargoLock.lockFile = ../../Cargo.lock;
  buildFeatures = [ "nixos" ];
  cargoBuildFlags = [ "-p" "nifty-filter" ];
  GIT_SHA = version;
  meta = {
    description = "A nifty tool to configure netfilter/nftables";
    license = lib.licenses.mit;
    mainProgram = "nifty-filter";
  };
}
