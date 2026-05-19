# Dashboard web UI for nifty-filter.
# SvelteKit frontend built with pnpm, Rust/Axum backend.
{ lib, stdenv, rustPlatform, pnpm, pnpmConfigHook, nodejs, pkg-config, openssl, fetchPnpmDeps, version ? "unknown" }:

let
  frontend = stdenv.mkDerivation {
    pname = "nifty-dashboard-frontend";
    version = "0.1.0";
    src = ../../crates/nifty-dashboard/frontend;
    nativeBuildInputs = [
      pnpm
      pnpmConfigHook
      nodejs
    ];
    pnpmDeps = fetchPnpmDeps {
      pname = "nifty-dashboard-frontend";
      version = "0.1.0";
      src = ../../crates/nifty-dashboard/frontend;
      hash = "sha256-mP9B4dpPYYci9LSejgz5jrkx85F4YKe06F9pm5gQD3U=";
      fetcherVersion = 3;
    };
    buildPhase = ''
      pnpm build
    '';
    installPhase = ''
      cp -r build $out
    '';
  };
in
rustPlatform.buildRustPackage {
  pname = "nifty-dashboard";
  version = "0.1.0";
  src = ../../crates/nifty-dashboard;
  cargoLock = {
    lockFile = ../../crates/nifty-dashboard/Cargo.lock;
    outputHashes = {
      "conf-0.4.5" = "sha256-gxxB8t0bl8ZudylXe4edAIVjO4KNHZshUhifvpm1b5E=";
    };
  };
  cargoBuildFlags = [ "-p" "nifty-dashboard" ];
  GIT_SHA = version;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];
  preBuild = ''
    rm -rf frontend/build
    ln -s ${frontend} frontend/build
    cp ${../../LICENSE.md} LICENSE.md
  '';
  meta = {
    description = "Web dashboard for nifty-filter";
    license = lib.licenses.mit;
    mainProgram = "nifty-dashboard";
  };
}
