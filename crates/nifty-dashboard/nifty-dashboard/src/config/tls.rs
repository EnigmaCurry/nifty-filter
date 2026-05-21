use std::path::PathBuf;

use conf::Conf;
use serde::{Deserialize, Serialize};

use crate::errors::CliError;

use std::{fmt, str::FromStr};

use super::StringList;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// No TLS – listen on plain HTTP only.
    #[default]
    None,
    /// Use local certificate and private key files.
    Manual,
    /// Use ACME (Let's Encrypt, Step-CA, etc.) for automatic TLS certificates.
    Acme,
}

impl fmt::Display for TlsMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TlsMode::None => "none",
            TlsMode::Manual => "manual",
            TlsMode::Acme => "acme",
        };
        write!(f, "{s}")
    }
}

impl FromStr for TlsMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(TlsMode::None),
            "manual" => Ok(TlsMode::Manual),
            "acme" => Ok(TlsMode::Acme),
            other => Err(format!(
                "invalid TLS mode '{other}', expected one of: none, manual, acme"
            )),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TlsAcmeChallenge {
    #[default]
    #[serde(rename = "tls_alpn_01")]
    TlsAlpn01,
    Http01,
    Dns01,
}

impl fmt::Display for TlsAcmeChallenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TlsAcmeChallenge::TlsAlpn01 => "tls-alpn-01",
            TlsAcmeChallenge::Http01 => "http-01",
            TlsAcmeChallenge::Dns01 => "dns-01",
        };
        write!(f, "{s}")
    }
}

impl FromStr for TlsAcmeChallenge {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tls-alpn-01" => Ok(TlsAcmeChallenge::TlsAlpn01),
            "http-01" => Ok(TlsAcmeChallenge::Http01),
            "dns-01" => Ok(TlsAcmeChallenge::Dns01),
            other => Err(format!(
                "invalid ACME challenge '{other}', expected one of: tls-alpn-01, http-01, dns-01"
            )),
        }
    }
}

#[derive(Conf, Debug, Clone, Serialize, Deserialize, Default)]
#[conf(serde)]
pub struct TlsConfig {
    /// TLS mode to use: none, manual, or acme.
    #[arg(long = "tls-mode", env = "TLS_MODE")]
    #[conf(default(TlsMode::None))]
    pub mode: TlsMode,

    /// Path to TLS certificate (PEM) when --tls-mode=manual.
    #[arg(long = "tls-cert", env = "TLS_CERT")]
    pub cert_path: Option<PathBuf>,

    /// Path to TLS private key (PEM) when --tls-mode=manual.
    #[arg(long = "tls-key", env = "TLS_KEY")]
    pub key_path: Option<PathBuf>,

    /// Additional DNS SubjectAltNames (SANs) for the TLS certificate.
    ///
    /// APP_HOST is used as the primary Common Name (CN); these names are added
    /// as SubjectAltNames. Used for ACME mode.
    #[arg(long = "tls-san", env = "TLS_SANS")]
    #[conf(default(StringList([].to_vec())))]
    pub sans: StringList,

    /// ACME challenge type to use when --tls-mode=acme.
    #[arg(long = "tls-acme-challenge", env = "TLS_ACME_CHALLENGE")]
    #[conf(default(TlsAcmeChallenge::TlsAlpn01))]
    pub acme_challenge: TlsAcmeChallenge,

    /// ACME directory URL (e.g. Step-CA or Let's Encrypt).
    /// Only used when --tls-mode=acme.
    #[arg(long = "tls-acme-directory-url", env = "TLS_ACME_DIRECTORY_URL")]
    #[conf(default("https://acme-v02.api.letsencrypt.org/directory".to_string()))]
    pub acme_directory_url: String,

    /// Contact email for ACME registration when --tls-mode=acme.
    #[arg(long = "tls-acme-email", env = "TLS_ACME_EMAIL")]
    pub acme_email: Option<String>,

    #[arg(long = "acme-dns-api-base", env = "ACME_DNS_API_BASE")]
    #[conf(default("https://auth.acme-dns.io".to_string()))]
    pub acme_dns_api_base: String,

    /// Path to client certificate PEM for mTLS (dashboard's own identity).
    #[arg(long = "tls-client-cert", env = "TLS_CLIENT_CERT")]
    pub client_cert_path: Option<PathBuf>,

    /// Path to client key PEM for mTLS.
    #[arg(long = "tls-client-key", env = "TLS_CLIENT_KEY")]
    pub client_key_path: Option<PathBuf>,
}

impl TlsConfig {
    pub fn validate_with_root(&self, _root_dir: &std::path::Path) -> Result<(), CliError> {
        // manual mode requirements
        if self.mode == TlsMode::Manual {
            if self.cert_path.is_none() || self.key_path.is_none() {
                return Err(CliError::InvalidArgs(
                    "Both --tls-cert and --tls-key are required when --tls-mode=manual."
                        .to_string(),
                ));
            }
        } else {
            if self.cert_path.is_some() || self.key_path.is_some() {
                return Err(CliError::InvalidArgs(
                    "--tls-cert/--tls-key are only valid when --tls-mode=manual.".to_string(),
                ));
            }
        }

        // Client cert/key must be provided together
        if self.client_cert_path.is_some() != self.client_key_path.is_some() {
            return Err(CliError::InvalidArgs(
                "--tls-client-cert and --tls-client-key must both be provided together."
                    .to_string(),
            ));
        }

        Ok(())
    }
}
