use log::{debug, error, info, warn};
use reqwest::Client;
use serde::Deserialize;

use crate::config::TechnitiumConfig;

const BASE_URL: &str = "http://localhost:5380";
const DEFAULT_USER: &str = "admin";
const DEFAULT_PASSWORD: &str = "admin";
const DEFAULT_ADDRESS: &str = "10.99.2.10";

#[derive(Deserialize)]
struct LoginResponse {
    status: String,
    token: Option<String>,
}

#[derive(Deserialize)]
struct TechnitiumApiResponse {
    status: String,
}

#[derive(Deserialize)]
struct ZoneListResponse {
    status: String,
    response: Option<ZoneListData>,
}

#[derive(Deserialize)]
struct ZoneListData {
    #[serde(default)]
    zones: Vec<ZoneEntry>,
}

#[derive(Deserialize)]
struct ZoneEntry {
    name: String,
}

/// Attempt to log in to Technitium, returning the session token on success.
async fn try_login(client: &Client, password: &str) -> Result<String, String> {
    let url = format!("{}/api/user/login", BASE_URL);
    let resp = client
        .post(&url)
        .form(&[("user", DEFAULT_USER), ("pass", password)])
        .send()
        .await
        .map_err(|e| format!("login request failed: {e}"))?;

    let body: LoginResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse login response: {e}"))?;

    if body.status == "ok" {
        body.token
            .ok_or_else(|| "login succeeded but no token returned".to_string())
    } else {
        Err(format!("login failed (status: {})", body.status))
    }
}

/// Log in with the configured password (or default on fresh install).
/// Returns (token, password_was_default).
async fn login(client: &Client, configured_password: &str) -> Result<(String, bool), String> {
    match try_login(client, DEFAULT_PASSWORD).await {
        Ok(token) => Ok((token, true)),
        Err(_) => {
            let token = try_login(client, configured_password).await?;
            Ok((token, false))
        }
    }
}

/// Change the admin password using an existing session token.
async fn change_password(
    client: &Client,
    token: &str,
    current: &str,
    new: &str,
) -> Result<(), String> {
    let url = format!("{}/api/user/changePassword", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("pass", current), ("newPass", new)])
        .send()
        .await
        .map_err(|e| format!("change password request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse change password response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("change password failed (status: {})", body.status))
    }
}

/// Check if a zone exists.
async fn zone_exists(client: &Client, token: &str, zone: &str) -> Result<bool, String> {
    let url = format!("{}/api/zones/list", BASE_URL);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("zone list request failed: {e}"))?;

    let body: ZoneListResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse zone list response: {e}"))?;

    if body.status != "ok" {
        return Err(format!("zone list failed (status: {})", body.status));
    }

    Ok(body
        .response
        .map(|r| r.zones.iter().any(|z| z.name == zone))
        .unwrap_or(false))
}

/// Create a primary zone.
async fn create_zone(client: &Client, token: &str, zone: &str) -> Result<(), String> {
    let url = format!("{}/api/zones/create", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[("zone", zone), ("type", "Primary")])
        .send()
        .await
        .map_err(|e| format!("zone create request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse zone create response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("zone create failed (status: {})", body.status))
    }
}

/// Add or overwrite an A record.
async fn ensure_a_record(
    client: &Client,
    token: &str,
    domain: &str,
    address: &str,
) -> Result<(), String> {
    let url = format!("{}/api/zones/records/add", BASE_URL);
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .form(&[
            ("domain", domain),
            ("type", "A"),
            ("ipAddress", address),
            ("overwrite", "true"),
            ("ttl", "3600"),
        ])
        .send()
        .await
        .map_err(|e| format!("record add request failed: {e}"))?;

    let body: TechnitiumApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse record add response: {e}"))?;

    if body.status == "ok" {
        Ok(())
    } else {
        Err(format!("record add failed (status: {})", body.status))
    }
}

/// Extract the parent zone from a domain (e.g. "dns.infra.lan" -> "infra.lan").
fn parent_zone(domain: &str) -> Option<&str> {
    domain.find('.').map(|i| &domain[i + 1..])
}

/// Tracks which configuration has been applied so we don't repeat work.
#[derive(Default)]
pub struct TechnitiumState {
    password_configured: bool,
    zone_configured: bool,
    /// Remember what domain+address we configured so we re-apply on change.
    last_domain: Option<String>,
    last_address: Option<String>,
}

/// Apply all Technitium configuration.
pub async fn apply(client: &Client, config: &TechnitiumConfig, state: &mut TechnitiumState) {
    let password = match config.admin_password.as_deref() {
        Some(p) if !p.is_empty() => p,
        _ => {
            debug!("no admin_password configured for technitium, skipping");
            return;
        }
    };

    // Log in (handles both fresh install and configured password).
    let (token, is_default) = match login(client, password).await {
        Ok(result) => result,
        Err(e) => {
            warn!("technitium: cannot log in: {e}");
            state.password_configured = false;
            state.zone_configured = false;
            return;
        }
    };

    // Handle password change on fresh install.
    if is_default && password != DEFAULT_PASSWORD {
        info!("technitium: fresh install detected, changing admin password");
        match change_password(client, &token, DEFAULT_PASSWORD, password).await {
            Ok(()) => {
                info!("technitium: admin password changed successfully");
                state.password_configured = true;
            }
            Err(e) => {
                error!("technitium: failed to change password: {e}");
                return;
            }
        }
        // Re-login with new password to get a valid token.
        // (The old token may be invalidated after password change.)
        return;
    }

    if !state.password_configured {
        if is_default {
            info!("technitium: admin password is the default (configured password matches default)");
        } else {
            info!("technitium: admin password already configured");
        }
        state.password_configured = true;
    }

    // Ensure zone and A record for the configured domain.
    let domain = match config.domain.as_deref() {
        Some(d) if !d.is_empty() => d,
        _ => return,
    };
    let address = config
        .address
        .as_deref()
        .unwrap_or(DEFAULT_ADDRESS);

    // Detect config changes.
    let domain_changed = state.last_domain.as_deref() != Some(domain);
    let address_changed = state.last_address.as_deref() != Some(address);
    if state.zone_configured && !domain_changed && !address_changed {
        return;
    }

    let zone = match parent_zone(domain) {
        Some(z) => z,
        None => {
            warn!("technitium: domain '{domain}' has no parent zone, skipping zone setup");
            return;
        }
    };

    // Ensure zone exists.
    match zone_exists(client, &token, zone).await {
        Ok(true) => debug!("technitium: zone '{zone}' already exists"),
        Ok(false) => {
            info!("technitium: creating zone '{zone}'");
            if let Err(e) = create_zone(client, &token, zone).await {
                error!("technitium: failed to create zone: {e}");
                return;
            }
        }
        Err(e) => {
            warn!("technitium: failed to check zone: {e}");
            return;
        }
    }

    // Ensure A record.
    info!("technitium: setting {domain} -> {address}");
    match ensure_a_record(client, &token, domain, address).await {
        Ok(()) => {
            info!("technitium: DNS record configured successfully");
            state.zone_configured = true;
            state.last_domain = Some(domain.to_string());
            state.last_address = Some(address.to_string());
        }
        Err(e) => error!("technitium: failed to set DNS record: {e}"),
    }
}
