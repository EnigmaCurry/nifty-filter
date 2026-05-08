use super::Cli;

pub fn build_log_level(cli: &Cli) -> String {
    match cli.verbose {
        0 => {
            // no -v: use --log, then env, then default
            cli.log
                .clone()
                .or_else(|| std::env::var("RUST_LOG").ok())
                .unwrap_or_else(|| "warn".to_string())
        }
        1 => {
            // -v: only this crate at info
            let crate_name = env!("CARGO_CRATE_NAME");
            format!("{crate_name}=info")
        }
        2 => {
            // -vv: only this crate at debug
            let crate_name = env!("CARGO_CRATE_NAME");
            format!("{crate_name}=debug")
        }
        3 => {
            // -vvv: global debug
            "debug".to_string()
        }
        _ => {
            // -vvvv or more: global trace
            "trace".to_string()
        }
    }
}
