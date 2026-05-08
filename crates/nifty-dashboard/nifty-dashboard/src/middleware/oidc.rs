use axum::http::Uri;
use axum_oidc::{EmptyAdditionalClaims, OidcAuthLayer};
use std::str::FromStr;

use crate::errors::CliError;

/// Config for OIDC
#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub enabled: bool,
    pub host_port: Option<String>,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ValidOidcConfig {
    pub net_host: Uri,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

impl OidcConfig {
    /// If enabled, parse/validate and return a ready-to-use config.
    /// If disabled, return Ok(None).
    pub fn validate(&self) -> Result<Option<ValidOidcConfig>, CliError> {
        if !self.enabled {
            return Ok(None);
        }

        let net_host_str = "https://".to_string()
            + self
                .host_port
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    CliError::InvalidArgs("Missing --net-host (required for oidc)".to_string())
                })?;

        let net_host = Uri::from_str(&net_host_str).map_err(|e| {
            CliError::InvalidArgs(format!("Invalid net_host URI '{net_host_str}': {e}"))
        })?;

        let issuer_raw = self
            .issuer
            .as_deref()
            .ok_or_else(|| CliError::InvalidArgs("Missing --auth-oidc-issuer".to_string()))?;
        let issuer = normalize_oidc_issuer(issuer_raw)?;

        let client_id = self
            .client_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| CliError::InvalidArgs("Missing --auth-oidc-client-id".to_string()))?
            .to_string();

        let client_secret = self
            .client_secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(Some(ValidOidcConfig {
            net_host,
            issuer,
            client_id,
            client_secret,
        }))
    }
}

fn normalize_oidc_issuer(raw: &str) -> Result<String, CliError> {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return Err(CliError::InvalidArgs("Missing oidc_issuer".to_string()));
    }

    // If user omitted scheme, assume https://
    if !s.contains("://") {
        s = format!("https://{s}");
    }

    // If user gave http://, normalize to https://
    if let Some(rest) = s.strip_prefix("http://") {
        s = format!("https://{rest}");
    }

    // Validate it's a plausible absolute URI with scheme + authority
    let uri: axum::http::Uri = s
        .parse()
        .map_err(|e| CliError::InvalidArgs(format!("Invalid oidc_issuer '{raw}': {e}")))?;

    let scheme = uri.scheme_str().ok_or_else(|| {
        CliError::InvalidArgs(format!("Invalid oidc_issuer '{raw}': missing scheme"))
    })?;

    if scheme != "https" {
        return Err(CliError::InvalidArgs(format!(
            "Invalid oidc_issuer '{raw}': scheme must be https"
        )));
    }

    if uri.authority().is_none() {
        return Err(CliError::InvalidArgs(format!(
            "Invalid oidc_issuer '{raw}': missing host"
        )));
    }

    // Ensure exactly one trailing slash
    while s.ends_with('/') {
        s.pop();
    }
    s.push('/');

    Ok(s)
}

pub async fn build_oidc_auth_layer(
    cfg: &OidcConfig,
) -> Result<Option<OidcAuthLayer<EmptyAdditionalClaims>>, CliError> {
    let v = match cfg.validate()? {
        Some(v) => v,
        None => return Ok(None), // only possible when enabled == false
    };

    let scopes = vec!["profile".to_string(), "email".to_string()];

    let layer = OidcAuthLayer::<EmptyAdditionalClaims>::discover_client(
        v.net_host,
        v.issuer,
        v.client_id,
        v.client_secret,
        scopes,
    )
    .await
    .map_err(|e| CliError::RuntimeError(format!("OIDC discovery failed: {e:#}")))?;

    Ok(Some(layer))
}
