# nifty-dashboard

<!-- [![Crates.io](https://img.shields.io/crates/v/nifty-dashboard?color=blue -->
<!-- )](https://crates.io/crates/nifty-dashboard) -->
[![Coverage](https://img.shields.io/badge/Coverage-Report-purple)](https://enigmacurry.github.io/nifty-dashboard/coverage/master/)

This is ALPHA software in-development.

## Features

 * Single binary deployment.
 * Embedded SQLite database.
 * RESTFul JSON API built with
   [axum](https://github.com/tokio-rs/axum).
 * Built-in TLS with the following modes:
   * ACME supporting TLS-ALPN-01 and DNS-01 challenge types. (e.g.,
     when you need a production certificate from Let's Encrypt.)
   * Automatic TLS with self-signed certificate (e.g., when using
     certificate pinning).
   * TLS with a provided certificate and key file (e.g., `.pem` files
     that you rotate manually).
   * None (plain HTTP) (e.g., when deployed behind a reverse proxy
     that terminates TLS on its behalf).
 * OpenAPI specification built with
   [aide](https://github.com/tamasfe/aide/).
   * Interactive API docs with your choice of
     [Scalar](https://github.com/ScalaR/ScalaR),
     [Redoc](https://github.com/Redocly/redoc), or [Swagger
     UI](https://github.com/swagger-api/swagger-ui?tab=readme-ov-file).
 * Multiple user authentication backends:
   * Username / Password.
   * Forward Auth via trusted header (Traefik Proxy or compatible proxy layer).
   * OAuth (OIDC).
 * Admin web interface.
 * [Just](https://github.com/casey/just) enabled project build
   targets.
 * "12 factor" style configuration with [Conf](https://github.com/cbeck88/conf-rs)
   * CLI argument parser.
   * Enviornment variable parser.
   * Config file parser.
   * Factory defaults.
   * Shell (tab) completion support for Bash / Fish / Zsh.
 * GitHub actions for tests and releases:
   * Test coverage report published to GitHub pages.
   * Builds executables for multiple platforms.
   * Builds Docker images for X86_64 and AArch64.
   * Publishing crates to crates.io (disabled by default, uncomment in
   [release.yml](template/.github/workflows/release.yml)).

## Install from source code

 * Install Rust with [rustup](https://rustup.rs/).
 * Install
   [Just](https://github.com/casey/just?tab=readme-ov-file#installation)
   (`cargo install just`)
 * Clone this git repository to your workstation.

```
just build --release
```

Find the built executable in `./target/release/nifty-dashboard`. You can
`install` it globally on your system:

```
sudo install \
  target/release/nifty-dashboard \
  /usr/local/bin/nifty-dashboard
```

## Install from binary release

This project is automatically built and released by GitHub actions.
Each git tag of the format `vX.X.X` will trigger the
[release.yml](.github/workflows/release.yml) action. 

To make your first release, create the git tag `v0.1.0` and push it.
It is required that the tag exactly matches the package version in
[nifty-dashboard/Cargo.toml](nifty-dashboard/Cargo.toml).

For future releases, you should use the `just bump-version` and `just
release` targets (See [DEVELOPMENT.md](DEVELOPMENT.md)). These commands
will automate the steps needed to cleanly upgrade the Cargo versions
and to create a release branch + pull request.

Binaries for Linux X86_64 and AArch64 are built and included in each
release:

 * [Download the latest release](https://github.com/enigmacurry/nifty-dashboard/releases/latest)

Docker images built for X86_64 and AArch64 are published on the GitHub
container registry (`ghcr.io`):

 * [Pull the latest Docker image](https://github.com/enigmacurry/nifty-dashboard/pkgs/container/nifty-dashboard)
 
## Configuration

The application uses a multi-source configuration system, consisting
of the following layers (from highest to lowest priority):

 1. **Command line arguments**. Every configuration setting has a long
    form CLI argument (e.g., `--some-setting foo`). Explicit args like
    this have the highest priority and will override the same setting
    from all other layers.

 2. **Environment variables**. Every configuration setting has an
    associated environment variable with categorization prefix (e.g,
    `CATEGORY_SOME_SETTING`). This is the preferred configuration
    style for Docker containers.

 3. **User Defaults**. The application has an optional config file in
    it's data root (`defaults.toml`). This file dynamically overrides
    the application's *default* settings and help messages.

 4. **Application defaults**. Every configuration setting has a
    default value compiled into the binary, used as a last resort.

### Application storage (stateful data)

The application needs a place to store its SQLite database files, ACME
accounts, and TLS certificates. By default, the application creates
files in `${XDG_DATA_HOME}/nifty-dashboard`, or `${HOME}/.local/share/nifty-dashboard`
(if no `XDG_DATA_HOME` is set) or `./nifty-dashboard-data` (if no `HOME`
variable is set).

If you want to use a different path, or if you want to support
multiple instances of the app, you need to override the path using the
command line argument `-C PATH` or `--root-dir PATH`.

## Run

Run `nifty-dashboard --help` to find all of the available options. Here are a
few examples of how you can run it:

### Plain HTTP

You should always use TLS, so only use plain HTTP if you are hosting
behind a reverse proxy that terminates TLS for you:

```
nifty-dashboard serve -v \
  --net-host           nifty-dashboard.example.org \
  --net-listen-ip      0.0.0.0 \
  --net-listen-port    8000 \
  --auth-method        username_password \
```

### Automatic self-signed TLS

```
nifty-dashboard serve -v \
  --net-host               nifty-dashboard.example.org \
  --net-listen-ip          0.0.0.0 \
  --net-listen-port        8443 \
  --auth-method            username_password \
  --tls-mode               self-signed
```

Note: self-signed certificates are not trusted in normal web browsers.
Use Manual TLS or ACME for production.

The above command will cache the self-signed CA and certificate files
in your data directory. If you would rather create an ephemeral
certificate, add the `--tls-self-signed-ephemeral` flag.

### Manual TLS

```
nifty-dashboard serve -v \
  --net-host           nifty-dashboard.example.org \
  --net-listen-ip      0.0.0.0 \
  --net-listen-port    8000 \
  --auth-method        username_password \
  --tls-mode           manual \
  --tls-cert           /path/to/some/cert.pem \
  --tls-key            /path/to/some/key.pem
```

### ACME (TLS-ALPN-01)

```
nifty-dashboard serve -v \
  --net-host               nifty-dashboard.example.org \
  --net-listen-ip          0.0.0.0 \
  --net-listen-port        443 \
  --auth-method            username_password \
  --tls-mode               acme \
  --tls-acme-challenge     tls-alpn-01 \
  --tls-acme-directory-url https://acme-v02.api.letsencrypt.org/directory \
  --tls-acme-email         ""
```

Note: TLS-ALPN-01 only work on port 443. So you need to run as `root`.

### ACME (DNS-01 via ACME-DNS)

```
## Register your ACME-DNS account. 
## Specify all of your domains (SANS) to get help with the CNAME records:
nifty-dashboard acme-dns-register \
  --acme-dns-api-base      https://auth.acme-dns.io \
  --net-host  nifty-dashboard.example.org \
  --tls-san ""

## Follow the directions in the output of acme-dns-register.
## Create the CNAME records it suggests for your domain.
## Run the `dig` command it suggests to verify the records.

## Loads ACME-DNS credentials and provisions cert on first run:
nifty-dashboard serve -v \
  --net-host               nifty-dashboard.example.org \
  --net-listen-ip          0.0.0.0 \
  --net-listen-port        8443 \
  --auth-method            username_password \
  --tls-mode               acme \
  --tls-acme-challenge     dns-01 \
  --tls-acme-directory-url https://acme-v02.api.letsencrypt.org/directory \
  --acme-dns-api-base      https://auth.acme-dns.io
```

See optional fields: `--tls-acme-email` if you want to set your ACME
account email address, `--tls-san` if you want additional SAN records
for your certificate.

## Write config file

If you would like to transform your command line parameters and
environment variables into a config file, you may use the `config`
command.

Here is the same ACME-DNS example as before, except instead of the
`serve` command, it is using the `config` command:

```
nifty-dashboard config -v \
  --net-host               nifty-dashboard.example.org \
  --net-listen-ip          0.0.0.0 \
  --net-listen-port        8443 \
  --auth-method            username_password \
  --tls-mode               acme \
  --tls-acme-challenge     dns-01 \
  --tls-acme-directory-url https://acme-v02.api.letsencrypt.org/directory \
  --acme-dns-api-base      https://auth.acme-dns.io
```

This outputs the TOML configuration to stdout:

```
## Example nifty-dashboard config ::
## (Write this to ~/.local/share/nifty-dashboard/defaults.toml)
## CLI options and env vars will always supercede this file.

[network]
listen_ip = "0.0.0.0"
listen_port = 8443
host = "nifty-dashboard.example.org"

[session]
check_seconds = 60
expiry_seconds = 60480

[auth]
method = "UsernamePassword"

[tls]
mode = "Acme"
sans = []
acme_challenge = "Dns01"
acme_directory_url = "https://acme-v02.api.letsencrypt.org/directory"
self_signed_ephemeral = false
acme_dns_api_base = "https://auth.acme-dns.io"
```

If you write this to `~/.local/share/nifty-dashboard/defaults.toml`, then you
can drop all the command line arguments and then just run `nifty-dashboard
serve` to use it.

## Development

See [DEVELOPMENT.md](DEVELOPMENT.md)

## Shell completion

To set up tab completion in your favorite shell:

### Bash

Put this in your `~/.bashrc` or similar:

```
## Enable completion for Bash:
source <(nifty-dashboard completions bash)
```

### Zsh

Put this in your `~/.zshrc` or similar:

```
# Enable completion for Zsh:
source <(nifty-dashboard completions zsh)
```

### Fish

Put this in your `~/.config/fish/config.fish` or similar:

```
# Enable completion for Fish:
nifty-dashboard completions fish > ~/.config/fish/completions/nifty-dashboard.fish
```

## Diff current project with the template

Sometimes it's useful to show all of the changes to the project since
the template was initialized. When you created the project from the
template, it created the first commit containing all of the template
files. You can use git diff to figure out the changeset. The Justfile
wraps these commands for you:

```
## List only the names of the files added/modified since init:
just template-changelog

## List the full diff between the current state and first commit:
just template-diff

## List the differences of a couple of files:
just template-diff Cargo.toml README.md
```
