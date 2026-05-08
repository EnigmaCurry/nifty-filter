use tracing_subscriber::EnvFilter;

pub fn init_tracing(log_level: &str) {
    let filter = EnvFilter::try_new(log_level)
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .pretty()
        .try_init()
        .ok();
}
