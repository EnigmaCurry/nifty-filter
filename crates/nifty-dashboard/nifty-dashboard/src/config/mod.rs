pub mod acme;
pub use acme::AcmeDnsRegisterConfig;
pub mod auth;
pub use auth::AuthConfig;
pub mod cli;
use cli::write_conf_error;
pub use cli::{Cli, Commands};
pub mod database;
pub use database::DatabaseConfig;
pub mod network;
pub use network::NetworkConfig;
pub mod serve;
pub use serve::ServeConfig;
pub mod session;
pub use session::SessionConfig;
pub mod tls;
pub use tls::{TlsAcmeChallenge, TlsConfig, TlsMode};
pub mod log;

use anyhow::Context;
use conf::Conf;
pub use log::build_log_level;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::info;

const DEFAULT_CONFIG_BASENAME: &str = "defaults.toml";

use serde::{Deserialize, Serialize};
use std::{fmt, ops::Deref, str::FromStr};

use crate::errors::CliError;
use crate::util::write_files::create_private_dir_all_0700_sync;

#[derive(Conf, Debug, Clone, Serialize, Deserialize)]
#[conf(serde)]
pub struct AppConfig {
    #[conf(flatten)]
    pub network: NetworkConfig,
    #[conf(flatten)]
    pub database: DatabaseConfig,
    #[conf(flatten)]
    pub session: SessionConfig,
    #[conf(flatten)]
    pub auth: AuthConfig,
    #[conf(flatten)]
    pub tls: TlsConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StringList(pub Vec<String>);

impl FromStr for StringList {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let items = s
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        Ok(StringList(items))
    }
}

impl fmt::Display for StringList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.join(","))
    }
}

impl Deref for StringList {
    type Target = [String];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) fn resolve_config_path(cli: &Cli, root_dir: &Path) -> PathBuf {
    if let Some(p) = &cli.config_file {
        return if p.is_relative() {
            root_dir.join(p)
        } else {
            p.to_path_buf()
        };
    }

    root_dir.join(DEFAULT_CONFIG_BASENAME)
}

pub(crate) fn load_toml_doc(path: &Path) -> Result<toml::Value, CliError> {
    let s = std::fs::read_to_string(path).map_err(|e| {
        CliError::RuntimeError(format!(
            "Could not read config file {}: {e}",
            path.display()
        ))
    })?;

    toml::from_str(&s).map_err(|e| {
        CliError::RuntimeError(format!(
            "Config file {} is not valid TOML: {e}",
            path.display()
        ))
    })
}

fn default_config_header() -> String {
    let bin = env!("CARGO_BIN_NAME");
    let default_root = default_root_dir();

    format!(
        r#"## Configuration in {bin} is hierarchical:
## (From highest priorirty to lowest priority):
##  1. Command line arguments (e.g., `{bin} serve --some-setting ...` ).
##  2. Environment variables (e.g., `export AXUM_DEV_SOME_SETTING=...` ).
##  3. Defaults file (this file) - you can change these settings in TOML format.
##  4. Compiled builtin defaults - to change these you have to recompile the source code.

## `{}` is the app's default root directory.
## By default, `{}` is loaded from within the app's root directory.

##  You can change the default data directory and/or defaults file in two ways:
##
##  1. Use your own data directory with `-C` (`--root-directory`).
##     This will find an optional `{}` file colocated in the same directory:
##       {bin} -C ~/prod/{bin}
##
##  2. Specify both the data directory (`-C`) and the config file path (`-f` or `--config`).
##     This will allow you to keep the data and config in separate directories:
##
##       {bin} -C ~/.local/share/{bin} -f /path/to/some/other/config.toml

## TOML example:
# [network]
# listen_ip = "127.0.0.2"
# listen_port = 3002
#
# [auth]
# method = "UsernamePassword"
"#,
        default_root.display(),
        DEFAULT_CONFIG_BASENAME,
        DEFAULT_CONFIG_BASENAME,
    )
}

pub(crate) fn handle_conf_err<W1: Write, W2: Write>(
    e: conf::Error,
    out: &mut W1,
    err: &mut W2,
) -> Result<(), CliError> {
    write_conf_error(&e, out, err);
    if e.exit_code() == 0 {
        Ok(())
    } else {
        Err(CliError::InvalidArgs(e.to_string()))
    }
}

pub(crate) fn ensure_root_dir(root_dir: PathBuf) -> Result<PathBuf, CliError> {
    if let Err(e) = create_private_dir_all_0700_sync(&root_dir) {
        return Err(CliError::RuntimeError(format!(
            "Failed to create root dir {}: {e}",
            root_dir.display()
        )));
    }
    Ok(root_dir)
}

pub(crate) fn ensure_config_file_exists(cfg_path: &Path) -> Result<(), CliError> {
    let is_defaults = cfg_path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s == DEFAULT_CONFIG_BASENAME)
        .unwrap_or(false);

    // If basename is defaults.toml and it's missing: create it once with header.
    if is_defaults && !cfg_path.exists() {
        if let Some(parent) = cfg_path.parent() {
            create_private_dir_all_0700_sync(parent)
                .context(format!("data directory invalid: {}", parent.display()))?;
        }

        match OpenOptions::new()
            .write(true)
            .create_new(true) // never clobber
            .open(cfg_path)
        {
            Ok(mut f) => {
                let header = default_config_header();
                f.write_all(header.as_bytes()).map_err(|e| {
                    CliError::RuntimeError(format!(
                        "Error writing default config file {}: {e}",
                        cfg_path.display()
                    ))
                })?;
                info!("Created default config file: {}", cfg_path.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // raced; fine
            }
            Err(e) => {
                return Err(CliError::RuntimeError(format!(
                    "Error creating default config file {}: {e}",
                    cfg_path.display()
                )));
            }
        }
    }

    // If it still doesn't exist (user provided a non-default missing path): hard error.
    if !cfg_path.exists() {
        return Err(CliError::InvalidArgs(format!(
            "Config file does not exist: {}",
            cfg_path.display()
        )));
    }
    info!("Loading defaults from config file: {}", cfg_path.display());
    Ok(())
}

pub(crate) fn default_root_dir() -> PathBuf {
    let bin = env!("CARGO_BIN_NAME");

    if let Ok(xdg) = env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join(bin);
    }

    if let Ok(home) = env::var("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home).join(".local").join("share").join(bin);
    }

    PathBuf::from(format!("{bin}-data"))
}
