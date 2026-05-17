# Sodola managed switch CLI client.
# Configures VLANs and port assignments on Sodola SL-SWTGW218AS switches.
{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "sodola-switch";
  version = "0.1.0";
  src = ../../.;
  cargoLock.lockFile = ../../Cargo.lock;
  cargoBuildFlags = [ "-p" "sodola-switch" ];
  meta = {
    description = "Management client for Sodola SL-SWTGW218AS managed switch";
    license = lib.licenses.mit;
    mainProgram = "sodola-switch";
  };
}
