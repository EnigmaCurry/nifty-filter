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
    /// Use ACME (Let's Encrypt, etc.) for automatic TLS certificates.
    Acme,
    /// Use a self-signed certificate generated at startup.
    SelfSigned,
}

impl fmt::Display for TlsMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TlsMode::None => "none",
            TlsMode::Manual => "manual",
            TlsMode::Acme => "acme",
            TlsMode::SelfSigned => "self-signed",
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
            "self-signed" => Ok(TlsMode::SelfSigned),
            other => Err(format!(
                "invalid TLS mode '{other}', expected one of: none, manual, acme, self-signed"
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

impl TlsConfig {
    pub const DEFAULT_SELF_SIGNED_VALID_SECONDS: u32 = 60 * 60 * 24 * 365; // 1 year
    pub const DEFAULT_CA_CERT_VALID_SECONDS: u32 = 60 * 60 * 24 * 365 * 10; // 10 years

    pub fn effective_self_signed_valid_seconds(&self) -> u32 {
        self.self_signed_valid_seconds
            .unwrap_or(Self::DEFAULT_SELF_SIGNED_VALID_SECONDS)
    }

    pub fn effective_ca_cert_valid_seconds(&self) -> u32 {
        self.ca_cert_valid_seconds
            .unwrap_or(Self::DEFAULT_CA_CERT_VALID_SECONDS)
    }
}

#[derive(Conf, Debug, Clone, Serialize, Deserialize, Default)]
#[conf(serde)]
pub struct TlsConfig {
    /// TLS mode to use: none, manual, acme, or self-signed.
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
    /// as SubjectAltNames. Used for ACME and self-signed modes.
    #[arg(long = "tls-san", env = "TLS_SANS")]
    #[conf(default(StringList([].to_vec())))]
    pub sans: StringList,

    /// ACME challenge type to use when --tls-mode=acme.
    #[arg(long = "tls-acme-challenge", env = "TLS_ACME_CHALLENGE")]
    #[conf(default(TlsAcmeChallenge::TlsAlpn01))]
    pub acme_challenge: TlsAcmeChallenge,

    /// ACME directory URL (e.g. Let's Encrypt).
    /// Only used when --tls-mode=acme.
    #[arg(long = "tls-acme-directory-url", env = "TLS_ACME_DIRECTORY_URL")]
    #[conf(default("https://acme-v02.api.letsencrypt.org/directory".to_string()))]
    pub acme_directory_url: String,

    /// Contact email for ACME registration when --tls-mode=acme.
    #[arg(long = "tls-acme-email", env = "TLS_ACME_EMAIL")]
    pub acme_email: Option<String>,

    /// Use ephemeral self-signed cert (no CA cert, no tls cache directory).
    /// Only valid when --tls-mode=self-signed.
    #[arg(long = "tls-self-signed-ephemeral", env = "TLS_SELF_SIGNED_EPHEMERAL")]
    pub self_signed_ephemeral: bool,

    /// Validity in seconds for the *leaf* certificate when --tls-mode=self-signed.
    /// If omitted, defaults to 1 year.
    #[arg(
        long = "tls-self-signed-valid-seconds",
        env = "TLS_SELF_SIGNED_VALID_SECONDS"
    )]
    pub self_signed_valid_seconds: Option<u32>,

    /// Validity in seconds for the *CA* certificate when --tls-mode=self-signed (non-ephemeral).
    /// If omitted, defaults to 10 years.
    #[arg(long = "tls-ca-cert-valid-seconds", env = "TLS_CA_CERT_VALID_SECONDS")]
    pub ca_cert_valid_seconds: Option<u32>,

    #[arg(long = "acme-dns-api-base", env = "ACME_DNS_API_BASE")]
    #[conf(default("https://auth.acme-dns.io".to_string()))]
    pub acme_dns_api_base: String,
}

impl TlsConfig {
    pub fn validate_with_root(&self, _root_dir: &std::path::Path) -> Result<(), CliError> {
        // -------------------------------
        // manual mode requirements
        // -------------------------------
        if self.mode == TlsMode::Manual {
            if self.cert_path.is_none() || self.key_path.is_none() {
                return Err(CliError::InvalidArgs(
                    "Both --tls-cert and --tls-key are required when --tls-mode=manual."
                        .to_string(),
                ));
            }
        } else {
            // Best practice: reject manual-only args outside manual mode
            if self.cert_path.is_some() || self.key_path.is_some() {
                return Err(CliError::InvalidArgs(
                    "--tls-cert/--tls-key are only valid when --tls-mode=manual.".to_string(),
                ));
            }
        }

        // -------------------------------
        // self-signed knobs require tls-mode=self-signed
        // -------------------------------
        if self.mode != TlsMode::SelfSigned {
            if self.self_signed_ephemeral {
                return Err(CliError::InvalidArgs(
                    "--tls-self-signed-ephemeral requires --tls-mode=self-signed.".to_string(),
                ));
            }
            if self.self_signed_valid_seconds.is_some() {
                return Err(CliError::InvalidArgs(
                    "--tls-self-signed-valid-seconds requires --tls-mode=self-signed.".to_string(),
                ));
            }
            if self.ca_cert_valid_seconds.is_some() {
                return Err(CliError::InvalidArgs(
                    "--tls-ca-cert-valid-seconds requires --tls-mode=self-signed.".to_string(),
                ));
            }
            return Ok(());
        }

        // -------------------------------
        // self-signed mode constraints
        // -------------------------------
        let leaf_secs = self.effective_self_signed_valid_seconds();

        // keep this >= your renewal logic sanity (and avoids ridiculous configs)
        if leaf_secs < 10 {
            return Err(CliError::InvalidArgs(format!(
                "--tls-self-signed-valid-seconds must be at least 10 (got {leaf_secs})."
            )));
        }

        if self.self_signed_ephemeral {
            // ephemeral: no CA, so CA validity must not be set
            if self.ca_cert_valid_seconds.is_some() {
                return Err(CliError::InvalidArgs(
                    "--tls-ca-cert-valid-seconds is not valid with --tls-self-signed-ephemeral (ephemeral mode has no CA)."
                        .to_string(),
                ));
            }
            return Ok(());
        }

        // non-ephemeral: CA is required and must outlive leaf
        let ca_secs = self.effective_ca_cert_valid_seconds();

        if ca_secs < 60 {
            return Err(CliError::InvalidArgs(format!(
                "--tls-ca-cert-valid-seconds must be at least 60 (got {ca_secs})."
            )));
        }

        if ca_secs <= leaf_secs {
            return Err(CliError::InvalidArgs(format!(
                "--tls-ca-cert-valid-seconds ({ca_secs}) must be greater than --tls-self-signed-valid-seconds ({leaf_secs})."
            )));
        }

        Ok(())
    }
}
