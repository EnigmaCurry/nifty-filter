# Polling monitor that applies configuration to nifty infrastructure services.
# Fetches services config from the router API and configures Technitium, etc.
{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "nifty-service-monitor";
  version = "0.1.0";
  src = ../../.;
  cargoLock.lockFile = ../../Cargo.lock;
  cargoBuildFlags = [ "-p" "nifty-service-monitor" ];
  meta = {
    description = "Polling monitor that applies configuration to nifty infrastructure services";
    license = lib.licenses.mit;
    mainProgram = "nifty-service-monitor";
  };
}
