use conf::completion::write_completion;
use config::cli::{args_after_subcommand, normalize_verbosity_args, write_conf_error};
use config::{AppConfig, build_log_level, handle_conf_err};
use config::{
    ServeConfig, ensure_config_file_exists, ensure_root_dir, load_toml_doc, resolve_config_path,
};
use errors::CliError;
use logging::init_tracing;
use std::io::Write;

mod api_docs;
mod commands;
mod config;
mod errors;
mod frontend;
mod logging;
mod middleware;
mod models;
mod prelude;
mod response;
mod routes;
mod server;
mod tls;
mod util;

use crate::config::{Cli, Commands};
use prelude::*;

fn main() {
    if let Err(e) = run_cli(
        std::env::args_os(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    ) {
        error!("run_cli failed: {:?}", e);
        eprintln!("{e}");
        std::process::exit(1);
    }
}

pub fn run_cli<I, S, W1, W2>(args: I, out: &mut W1, _err: &mut W2) -> Result<(), CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
    W1: Write,
    W2: Write,
{
    use conf::Conf;

    // Normalize args to OsString so we can reason about length & reuse them.
    let args_vec: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();

    // If the user provided no subcommand (just the binary name),
    // pretend they ran `axum-dev --help` and print the same help output.
    // No subcommand => behave like --help, but write into `out`
    if args_vec.len() <= 1 {
        if let Err(e) = Cli::try_parse_from([env!("CARGO_BIN_NAME"), "--help"], std::env::vars_os())
        {
            write_conf_error(&e, out, _err);
        }
        return Ok(());
    }

    // Pre-process args for verbosity setting (allows extra extra verbose with `-vvv`)
    let args_vec = normalize_verbosity_args(args_vec);
    // Normal invocation:
    let cli = match Cli::try_parse_from(args_vec.clone(), std::env::vars_os()) {
        Ok(cli) => cli,
        Err(e) => {
            write_conf_error(&e, out, _err);
            if e.exit_code() == 0 {
                return Ok(());
            } else {
                return Err(CliError::InvalidArgs(e.to_string()));
            }
        }
    };

    // Your existing validation
    cli.validate()?;

    let log_level = build_log_level(&cli);
    init_tracing(&log_level);

    match cli.command {
        Commands::Completions(args) => {
            write_completion::<Cli, _>(args.shell, None, out)?;
            Ok(())
        }

        Commands::Config(ref _first_pass_cfg) => {
            let root_dir = ensure_root_dir(cli.root_dir.clone().0)?;
            trace!("Root directory (app data): {}", cli.root_dir);

            let cfg_path = resolve_config_path(&cli, &root_dir);
            ensure_config_file_exists(&cfg_path)?;

            // Build args for ServeConfig parse: everything after the "config" token.
            let config_args = args_after_subcommand(&args_vec, "config")
                .ok_or_else(|| CliError::InvalidArgs("Missing 'config' subcommand".to_string()))?;

            let doc = load_toml_doc(&cfg_path)?;
            let serve_cfg = match ServeConfig::conf_builder()
                .args(config_args)
                .env(std::env::vars_os())
                .doc(cfg_path.to_string_lossy(), doc)
                .try_parse()
            {
                Ok(cfg) => cfg,
                Err(e) => return handle_conf_err(e, out, _err),
            };

            // Optional: keep parity with `serve` by validating what would be used at runtime.
            let app_cfg: AppConfig = serve_cfg.app;
            app_cfg.tls.validate_with_root(&root_dir)?;
            app_cfg.auth.validate()?;

            // Print the effective config as TOML
            let rendered = toml::to_string_pretty(&app_cfg).map_err(|e| {
                CliError::InvalidArgs(format!("Failed to serialize config as TOML: {e}"))
            })?;

            let bin = env!("CARGO_BIN_NAME");
            out.write(&format!("## Example {bin} config ::\n").into_bytes())?;
            out.write(
                &format!("## (Write this to ~/.local/share/{bin}/defaults.toml)\n").into_bytes(),
            )?;
            out.write(b"## CLI options and env vars will always supercede this file.\n\n")?;

            out.write_all(rendered.as_bytes())?;
            if !rendered.ends_with('\n') {
                out.write_all(b"\n")?;
            }
            Ok(())
        }
        Commands::Serve(ref _first_pass_cfg) => {
            let root_dir = ensure_root_dir(cli.root_dir.clone().0)?;
            info!("Root directory (app data): {}", cli.root_dir);

            let cfg_path = resolve_config_path(&cli, &root_dir);
            ensure_config_file_exists(&cfg_path)?;

            // Build args for ServeConfig parse: everything after the "serve" token.
            let serve_args = args_after_subcommand(&args_vec, "serve")
                .ok_or_else(|| CliError::InvalidArgs("Missing 'serve' subcommand".to_string()))?;

            let doc = load_toml_doc(&cfg_path)?;
            let serve_cfg = match ServeConfig::conf_builder()
                .args(serve_args)
                .env(std::env::vars_os())
                .doc(cfg_path.to_string_lossy(), doc)
                .try_parse()
            {
                Ok(cfg) => cfg,
                Err(e) => return handle_conf_err(e, out, _err),
            };

            let app_cfg: AppConfig = serve_cfg.app;

            app_cfg.tls.validate_with_root(&root_dir)?;
            app_cfg.auth.validate()?;

            commands::serve(app_cfg, root_dir)
        }
        Commands::AcmeDnsRegister(args) => {
            commands::acme_dns_register(args, cli.root_dir.clone().0, out, _err)
        }
        Commands::Sql(args) => commands::sql(args, cli.root_dir.clone().0, out, _err),
    }
}

#[test]
fn help_prints_when_no_subcommand() {
    let mut out = Vec::new();
    let mut err = Vec::new();

    let bin = env!("CARGO_BIN_NAME");
    // No subcommand => run_cli should print top-level help to stdout and succeed.
    run_cli([bin], &mut out, &mut err).expect("run_cli should succeed for help");

    assert!(
        err.is_empty(),
        "expected no stderr output, got: {}",
        String::from_utf8_lossy(&err)
    );

    let actual = String::from_utf8(out).expect("stdout should be valid utf8");

    // Very loose assertion: just make sure it looks like help and mentions 'serve'.
    assert!(!actual.is_empty(), "help output is blank");
    assert!(
        actual.contains("Run the HTTP API server"),
        "help output did not mention the 'serve' subcommand.\nActual help:\n{actual}"
    );
}
