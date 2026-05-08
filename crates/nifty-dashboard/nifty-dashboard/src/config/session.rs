use conf::Conf;
use serde::{Deserialize, Serialize};

#[derive(Conf, Debug, Clone, Serialize, Deserialize)]
#[conf(serde)]
pub struct SessionConfig {
    /// Session cleanup interval in seconds.
    /// (default 60, or set SESSION_CHECK_SECONDS).
    #[arg(long = "session-check-seconds", env = "SESSION_CHECK_SECONDS")]
    #[conf(default(60))]
    pub check_seconds: u64,

    /// Session inactivity timeout in seconds.
    /// (default 604800 = 7 days, or set SESSION_EXPIRY_SECONDS).
    #[arg(long = "session-expiry-seconds", env = "SESSION_EXPIRY_SECONDS")]
    #[conf(default(60480))]
    pub expiry_seconds: u64,
}
