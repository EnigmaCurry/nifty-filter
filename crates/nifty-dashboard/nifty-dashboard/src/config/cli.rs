use std::ffi::OsString;
use std::{fmt, io::Write, path::PathBuf, str::FromStr};

use crate::{config::default_root_dir, errors::CliError};

use super::{AcmeDnsRegisterConfig, ServeConfig};
use conf::{Conf, Subcommands, anstyle::AnsiColor, completion::Shell};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct RootDir(pub PathBuf);

impl FromStr for RootDir {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(RootDir(PathBuf::from(s)))
    }
}

impl fmt::Display for RootDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Good enough for help text / env hint
        write!(f, "{}", self.0.to_string_lossy())
    }
}

const HELP_STYLES: conf::Styles = conf::Styles::styled()
    .header(AnsiColor::Blue.on_default().bold())
    .usage(AnsiColor::Blue.on_default().bold())
    .literal(AnsiColor::White.on_default())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Conf, Debug, Clone)]
#[conf(serde, styles = HELP_STYLES)]
pub struct Cli {
    /// Sets the log level, overriding the RUST_LOG environment variable.
    #[arg(long)]
    pub log: Option<String>,

    /// Increase verbosity. Can be specified multiple times like -vvvv
    #[arg(short = 'v')]
    #[conf(default(0u8))]
    pub verbose: u8,

    /// Base directory for config + state.
    #[arg(short = 'C', long = "root-dir", env = "ROOT_DIR")]
    #[conf(default(RootDir(default_root_dir())), serde(skip))]
    pub root_dir: RootDir,

    /// Config file (e.g. defaults.toml in ROOT_DIR)
    #[arg(short = 'f', long = "config", env = "CONFIG_FILE")]
    #[conf(serde(skip))]
    pub config_file: Option<PathBuf>,

    /// Subcommands.
    #[conf(subcommands)]
    pub command: Commands,
}

impl Cli {
    pub fn validate(&self) -> Result<(), CliError> {
        match &self.command {
            // we now validate Serve *after* merging config in run_cli
            Commands::Serve(_) => Ok(()),
            Commands::AcmeDnsRegister { .. } => Ok(()),
            Commands::Completions(_) => Ok(()),
            Commands::Config(_) => Ok(()),
            Commands::Sql(_) => Ok(()),
        }
    }
}

#[derive(Conf, Debug, Clone, Serialize, Deserialize)]
#[conf(serde)]
pub struct CompletionArgs {
    /// Shell to generate completions for (bash|elvish|fish|powershell|zsh)
    #[conf(pos, serde(skip))]
    pub shell: Shell,
}

#[derive(Subcommands, Debug, Clone)]
#[conf(serde)]
pub enum Commands {
    /// Output shell completion scripts
    Completions(CompletionArgs),

    /// Run the HTTP API server.
    Serve(ServeConfig),

    /// Output a config file using the current config as a template
    Config(ServeConfig),

    /// Register or inspect acme-dns credentials used for DNS-01 ACME.
    ///
    /// Run this once before `serve` when using `--tls-mode=acme --tls-acme-challenge=dns-01`,
    /// unless you are providing ACME_DNS_* credentials explicitly.
    AcmeDnsRegister(AcmeDnsRegisterConfig),

    /// Run SQLite shell with the application's database loaded
    Sql(ServeConfig),
}

pub(crate) fn write_conf_error<W1: Write, W2: Write>(e: &conf::Error, out: &mut W1, err: &mut W2) {
    // In clap, help/version typically exit with code 0 (stdout-y),
    // while real argument errors are nonzero (stderr-y).
    let mut msg = e.to_string();
    if !msg.ends_with('\n') {
        msg.push('\n');
    }

    if e.exit_code() == 0 {
        let _ = out.write_all(msg.as_bytes());
    } else {
        let _ = err.write_all(msg.as_bytes());
    }
}

pub(crate) fn args_after_subcommand(
    args: &[std::ffi::OsString],
    sub: &str,
) -> Option<Vec<std::ffi::OsString>> {
    let bin = args.first()?.clone();
    let idx = args.iter().position(|a| a.to_string_lossy() == sub)?;
    let mut out = Vec::with_capacity(1 + (args.len().saturating_sub(idx + 1)));
    out.push(bin);
    out.extend_from_slice(&args[idx + 1..]);
    Some(out)
}

pub fn normalize_verbosity_args<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    // Keep this list in sync with your Commands enum variants.
    const SUBCOMMANDS: &[&str] = &["serve", "config", "acme-dns-register", "completions"];

    let mut it = args.into_iter();

    // argv[0]
    let mut out: Vec<OsString> = Vec::new();
    let Some(bin) = it.next() else { return out };
    out.push(bin);

    let mut counted: u8 = 0;
    let mut explicit: Option<u8> = None;

    // Collect all non-verbosity args here (preserving order)
    let mut rest: Vec<OsString> = Vec::new();

    while let Some(arg) = it.next() {
        let s = arg.to_string_lossy();

        // -v
        if s == "-v" {
            // If next token is a number, treat as explicit "-v 2"
            if let Some(peek) = it.next() {
                if let Ok(n) = peek.to_string_lossy().parse::<u8>() {
                    explicit = Some(n);
                } else {
                    // Not a number => this is just a flag occurrence, keep the peek
                    counted = counted.saturating_add(1);
                    rest.push(peek);
                }
            } else {
                counted = counted.saturating_add(1);
            }
            continue;
        }

        // -vvv
        if s.starts_with('-') && s.len() > 2 && s[1..].chars().all(|c| c == 'v') {
            counted = counted.saturating_add((s.len() - 1).min(u8::MAX as usize) as u8);
            continue;
        }

        // -v=3
        if let Some(rest_num) = s.strip_prefix("-v=")
            && let Ok(n) = rest_num.parse::<u8>()
        {
            explicit = Some(n);
            continue;
        }

        // otherwise keep arg
        rest.push(arg);
    }

    let level = explicit.unwrap_or(counted);
    if level == 0 {
        out.extend(rest);
        return out;
    }

    // Find insertion point: right before the subcommand token (if present)
    let insert_at = rest
        .iter()
        .position(|a| {
            let a = a.to_string_lossy();
            SUBCOMMANDS.iter().any(|&sc| sc == a)
        })
        .unwrap_or(rest.len());

    out.extend(rest[..insert_at].iter().cloned());
    out.push(OsString::from("-v"));
    out.push(OsString::from(level.to_string()));
    out.extend(rest[insert_at..].iter().cloned());

    out
}
