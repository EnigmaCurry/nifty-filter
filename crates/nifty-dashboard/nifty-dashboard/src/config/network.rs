use conf::Conf;
use serde::{Deserialize, Serialize};

#[derive(Conf, Serialize, Deserialize, Debug, Clone)]
#[conf(serde)]
pub struct NetworkConfig {
    /// IP to bind (or set NET_LISTEN_IP).
    #[arg(long = "net-listen-ip", env = "NET_LISTEN_IP")]
    #[conf(default("127.0.0.1".to_string()))]
    pub listen_ip: String,

    /// Port to bind (or set NET_LISTEN_PORT).
    #[arg(long = "net-listen-port", env = "NET_LISTEN_PORT")]
    #[conf(default(3000))]
    pub listen_port: u16,

    /// Public-facing HTTPS port for redirects (when behind NAT/nftables redirect).
    /// Defaults to 443. The redirect from plain HTTP will point to this port.
    #[arg(long = "net-public-port", env = "NET_PUBLIC_PORT")]
    #[conf(default(443))]
    pub public_port: u16,

    /// Primary public hostname for this app (used as the default TLS CN).
    #[arg(long = "net-host", env = "NET_HOST")]
    pub host: Option<String>,
}
