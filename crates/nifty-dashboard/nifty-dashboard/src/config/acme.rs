use conf::Conf;
use serde::{Deserialize, Serialize};

use super::StringList;

#[derive(Conf, Debug, Clone, Serialize, Deserialize)]
#[conf(serde)]
pub struct AcmeDnsRegisterConfig {
    /// Base URL of the acme-dns API (e.g. https://auth.acme-dns.io).
    #[arg(long = "acme-dns-api-base", env = "ACME_DNS_API_BASE")]
    #[conf(default("https://auth.acme-dns.io".to_string()))]
    pub api_base: String,

    /// Optional CIDR ranges allowed to call the acme-dns /update API.
    ///
    /// This is passed through to the acme-dns `allowfrom` field.
    #[arg(long = "acme-dns-allow-from", env = "ACME_DNS_ALLOW_FROM")]
    #[conf(default(StringList(["0.0.0.0/0".to_string()].to_vec())))]
    pub allow_from: StringList,

    /// Primary public hostname.
    #[arg(long = "net-host", env = "NET_HOST")]
    pub host: Option<String>,

    /// Additional DNS SubjectAltNames (SANs) for the TLS certificate.
    ///
    /// APP_HOST is used as the primary Common Name (CN); these names are added
    /// as SubjectAltNames. Used for ACME and self-signed modes.
    #[arg(long = "tls-san", env = "TLS_SANS")]
    #[conf(default(StringList([].to_vec())))]
    pub sans: StringList,
}
