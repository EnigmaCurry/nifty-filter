use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

use acme_dns_client::{AcmeDnsClient, Credentials};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use instant_acme::{
    Account, AccountBuilder, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use serde_json;

use crate::util::write_files::create_private_dir_all_0700;

/// Abstraction over “something that can set TXT records for ACME DNS-01”.
///
/// The `domain` parameter is the ACME identifier (e.g. `example.com` or
/// `*.example.com`). Some providers (like acme-dns) don’t actually need it
/// to perform the update, but it’s useful for logging and for providers
/// that do direct DNS updates.
///
/// `value` is the TXT content the ACME client asks us to publish.
#[async_trait]
pub trait DnsChallengeProvider: Send + Sync {
    /// Publish/overwrite the TXT record for the given domain.
    async fn set_challenge_txt(&self, domain: &str, value: &str) -> anyhow::Result<()>;

    /// Optional cleanup hook after validation.
    ///
    /// Many providers (including acme-dns) don’t *need* cleanup; in those
    /// cases this can be a no-op.
    async fn clear_challenge_txt(&self, domain: &str) -> anyhow::Result<()>;
}

/// DNS-01 provider backed by joohoi/acme-dns via the `acme-dns-client` crate.
///
/// This assumes:
///   * `ACME_DNS_API_BASE` points at your acme-dns API, and
///   * `ACME_DNS_*` env vars contain account credentials (see acme-dns-client docs).
pub struct AcmeDnsProvider {
    client: AcmeDnsClient,
    creds: Credentials,
}

impl AcmeDnsProvider {
    /// Build an AcmeDnsProvider from the cached credentials file and an API base URL.
    ///
    /// - `api_base` should come from CLI/Env via clap (e.g. --acme-dns-api-base).
    /// - `cache_dir` is where `acme-dns-register` wrote acme-dns-credentials.json.
    ///
    /// This does **not** read any ACME_DNS_* env vars at runtime.
    pub async fn from_cache(api_base: &str, cache_dir: &Path) -> Result<Self> {
        let cred_path: PathBuf = cache_dir.join("acme-dns-credentials.json");

        if cred_path.exists() {
            tracing::info!(
                "acme-dns: loading cached credentials from '{}'",
                cred_path.display()
            );

            let data = tokio::fs::read(&cred_path).await.with_context(|| {
                format!(
                    "failed reading cached acme-dns credentials at {}",
                    cred_path.display()
                )
            })?;

            let creds: Credentials = serde_json::from_slice(&data)
                .context("failed to parse cached acme-dns credentials as JSON")?;

            let client = AcmeDnsClient::new(api_base)
                .with_context(|| format!("failed to construct AcmeDnsClient for {api_base}"))?;

            return Ok(Self { client, creds });
        }

        // No cache file → tell the user to run the bootstrap command.
        let bin = env!("CARGO_BIN_NAME");

        Err(anyhow!(
            "\n\nError: acme-dns credentials not found in '{}'.\n\n\
               - Run `{bin} acme-dns-register --acme-dns-api-base <URL>` first.\n\n\
             After that, re-run your `serve` command.\n\n",
            cache_dir.display()
        ))
    }

    pub fn into_shared(self) -> Arc<dyn DnsChallengeProvider> {
        Arc::new(self)
    }
}

#[async_trait]
impl DnsChallengeProvider for AcmeDnsProvider {
    async fn set_challenge_txt(&self, domain: &str, value: &str) -> anyhow::Result<()> {
        tracing::info!(
            "acme-dns: setting DNS-01 TXT challenge for domain={domain} via subdomain={}",
            self.creds.subdomain
        );
        self.client.update_txt(&self.creds, value).await?;
        Ok(())
    }

    async fn clear_challenge_txt(&self, domain: &str) -> anyhow::Result<()> {
        // acme-dns doesn’t really support per-challenge cleanup; TXT values
        // are just overwritten on the next challenge. We keep this for API
        // completeness and future providers that *do* delete records.
        tracing::debug!(
            "acme-dns: clear_challenge_txt({domain}) is a no-op (acme-dns keeps the last token)"
        );
        Ok(())
    }
}

/// Drive a DNS-01 ACME flow using `instant_acme` and a `DnsChallengeProvider`.
///
/// * Supports multiple domains in `domains` (we issue a SAN cert).
/// * For each authorization, we:
///   - pick the DNS-01 challenge,
///   - compute the TXT value from `key_authorization().dns_value()`,
///   - call `dns_provider.set_challenge_txt(...)`,
///   - call `challenge.set_ready()`.
///
/// After all challenges are “ready”, we:
///   - `poll_ready` on the order,
///   - `finalize` (which generates a private key),
///   - `poll_certificate` to get the chain.
///
/// Returns `(cert_pem_bytes, key_pem_bytes)`.
pub async fn obtain_certificate_with_dns01(
    directory_url: &str,
    contact_email: Option<&str>,
    domains: &[String],
    dns_provider: &dyn DnsChallengeProvider,
    cache_dir: &Path,
) -> Result<(Vec<u8>, Vec<u8>)> {
    if domains.is_empty() {
        return Err(anyhow!(
            "DNS-01 ACME requires at least one domain (identifier)"
        ));
    }

    // 1) Create / load ACME account.
    let account = create_or_load_account(directory_url, contact_email, cache_dir)
        .await
        .context("failed to create ACME account")?;

    // 2) Create order for all requested domains.
    let identifiers: Vec<Identifier> = domains.iter().map(|d| Identifier::Dns(d.clone())).collect();

    let mut order = account
        .new_order(&NewOrder::new(&identifiers))
        .await
        .context("failed to create ACME order")?;

    let state = order.state();
    tracing::info!("acme dns-01: initial order state: {state:#?}");
    if !matches!(state.status, OrderStatus::Pending | OrderStatus::Ready) {
        return Err(anyhow!(
            "unexpected initial order status for DNS-01: {:?}",
            state.status
        ));
    }

    // 3) For each authorization, solve the DNS-01 challenge.
    let mut authorizations = order.authorizations();

    // Keep track of published FQDNs for optional cleanup.
    let mut published_records: Vec<String> = Vec::new();

    while let Some(result) = authorizations.next().await {
        let mut authz = result.context("reading authorization")?;

        match authz.status {
            AuthorizationStatus::Pending => {
                // Need to solve this one.
            }
            AuthorizationStatus::Valid => {
                tracing::info!(
                    "acme dns-01: authorization already valid for {:?}",
                    authz.identifier()
                );
                continue;
            }
            other => {
                return Err(anyhow!(
                    "unsupported authorization status for DNS-01: {other:?}"
                ));
            }
        }

        // Pick DNS-01 challenge.
        let mut challenge = authz
            .challenge(ChallengeType::Dns01)
            .ok_or_else(|| anyhow!("no dns-01 challenge found for this authorization"))?;

        // Domain the CA is validating.
        let ident = challenge.identifier().to_string();
        // Wildcards must use the base name for the TXT record.
        let base = ident.trim_start_matches("*.");
        let fqdn = format!("_acme-challenge.{base}");

        // instant-acme gives us the DNS-01 TXT value directly.
        let txt_value = challenge.key_authorization().dns_value().to_string();

        tracing::info!(
            "acme dns-01: publishing TXT record {} = {}",
            fqdn,
            txt_value
        );

        dns_provider
            .set_challenge_txt(&fqdn, &txt_value)
            .await
            .context("failed to publish DNS-01 TXT record")?;

        published_records.push(fqdn.clone());

        // Tell the CA the challenge is ready to be validated.
        challenge
            .set_ready()
            .await
            .context("failed to notify ACME server that DNS-01 challenge is ready")?;
    }

    // 4) Exponentially back off until the order becomes Ready or Invalid.
    let status = order
        .poll_ready(&RetryPolicy::default())
        .await
        .context("polling ACME order readiness failed")?;

    if status != OrderStatus::Ready {
        let state = order.state();
        tracing::info!(
            "acme dns-01: order status after poll_ready = {:?}, error = {:?}",
            status,
            state.error
        );

        // If the server gave us a top-level ProblemDetail, surface that.
        if let Some(problem) = &state.error {
            return Err(anyhow!(
                "ACME order became {:?}: type={:?}, detail={:?}",
                status,
                problem.r#type,
                problem.detail,
            ));
        }

        // Optionally: walk authorizations and log their per-auth status / challenge errors.
        let mut authzs = order.authorizations();
        while let Some(res) = authzs.next().await {
            match res {
                Ok(authz) => {
                    // Summarize each challenge on this authorization.
                    let summaries: Vec<String> = authz
                        .challenges
                        .iter()
                        .map(|c| {
                            format!(
                                "type={:?}, status={:?}, error_type={:?}, error_detail={:?}",
                                c.r#type,
                                c.status,
                                c.error.as_ref().map(|e| &e.r#type),
                                c.error.as_ref().map(|e| &e.detail),
                            )
                        })
                        .collect();

                    if authz.status != AuthorizationStatus::Valid {
                        tracing::error!(
                            "acme dns-01: authz for {:?}: status={:?}, wildcard={:?}, challenges=[{}]",
                            authz.identifier(),
                            authz.status,
                            authz.wildcard,
                            summaries.join("; "),
                        );
                    } else {
                        tracing::info!(
                            "acme dns-01: authz for {:?}: status={:?}, wildcard={:?}, challenges=[{}]",
                            authz.identifier(),
                            authz.status,
                            authz.wildcard,
                            summaries.join("; "),
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("acme dns-01: failed to fetch authz state after Invalid: {e}");
                }
            }
        }

        return Err(anyhow!(
            "ACME DNS-01 order failed: status={:?} (no error detail from server)",
            status
        ));
    }

    // 5) Finalize the order.
    //
    // In 0.8.x, `finalize()` generates a fresh private key and returns it in PEM.
    let private_key_pem = order
        .finalize()
        .await
        .context("failed to finalize ACME order (DNS-01)")?;

    // 6) Retrieve the issued certificate chain (PEM).
    let cert_chain_pem = order
        .poll_certificate(&RetryPolicy::default())
        .await
        .context("failed to retrieve certificate chain from ACME server")?;

    // 7) Optional best-effort cleanup of TXT records (no-op for acme-dns).
    for fqdn in published_records {
        let _ = dns_provider.clear_challenge_txt(&fqdn).await.ok();
    }

    Ok((cert_chain_pem.into_bytes(), private_key_pem.into_bytes()))
}

async fn create_or_load_account(
    directory_url: &str,
    contact_email: Option<&str>,
    cache_dir: &Path,
) -> Result<Account> {
    use tokio::fs;

    let cred_path = cache_dir.join("acme-account-credentials.json");

    // --- Fast path: reuse existing account credentials ---
    if cred_path.exists() {
        tracing::info!(
            "acme dns-01: loading ACME account credentials from '{}'",
            cred_path.display()
        );

        let data = fs::read(&cred_path).await.with_context(|| {
            format!(
                "failed to read ACME account credentials from {}",
                cred_path.display()
            )
        })?;

        let creds: AccountCredentials = serde_json::from_slice(&data)
            .context("failed to parse ACME account credentials JSON")?;

        // Build a fresh AccountBuilder and restore the account from credentials
        let builder: AccountBuilder =
            Account::builder().context("failed to construct ACME AccountBuilder")?;

        let account = builder
            .from_credentials(creds)
            .await
            .context("failed to build ACME account from cached credentials")?;

        return Ok(account);
    }

    // --- Slow path: register a new account, then cache its credentials ---
    tracing::info!(
        "acme dns-01: no cached ACME account credentials at '{}'; registering new account",
        cred_path.display()
    );

    // Build the NewAccount payload.
    let mut contact_strings = Vec::new();
    if let Some(email) = contact_email
        && !email.is_empty()
    {
        contact_strings.push(format!("mailto:{email}"));
    }

    // instant-acme expects `&[&str]` here.
    let contact_refs: Vec<&str> = contact_strings.iter().map(|s| s.as_str()).collect();
    let new_account = NewAccount {
        contact: &contact_refs,
        terms_of_service_agreed: true,
        only_return_existing: false,
    };

    // Create a new account and get its credentials.
    let builder: AccountBuilder =
        Account::builder().context("failed to construct ACME AccountBuilder")?;

    let (account, creds) = builder
        .create(&new_account, directory_url.to_owned(), None)
        .await
        .context("failed to register new ACME account")?;

    // Persist credentials for reuse on next run.
    let json = serde_json::to_vec_pretty(&creds)
        .context("failed to serialize ACME account credentials to JSON")?;

    create_private_dir_all_0700(cache_dir)
        .await
        .with_context(|| {
            format!(
                "failed to create ACME cache dir {} for account credentials",
                cache_dir.display()
            )
        })?;

    fs::write(&cred_path, &json).await.with_context(|| {
        format!(
            "failed to write ACME account credentials to {}",
            cred_path.display()
        )
    })?;

    Ok(account)
}

/// Register (or reuse) an acme-dns account and persist credentials to `cache_dir`.
///
/// - If the credentials file already exists, it is loaded and returned with `created_new = false`.
/// - Otherwise, a new account is registered, persisted, and returned with `created_new = true`.
pub async fn register_acme_dns_account(
    api_base: &str,
    cache_dir: &Path,
    _domains: &[String],
    allow_from: Option<&[String]>,
) -> Result<(Credentials, bool)> {
    // Ensure cache dir exists
    create_private_dir_all_0700(cache_dir)
        .await
        .with_context(|| format!("failed to create tls cache dir '{}'", cache_dir.display()))?;

    let cred_path = cache_dir.join("acme-dns-credentials.json");

    // If we already have creds, just load and return them.
    if cred_path.exists() {
        tracing::info!(
            "acme-dns: credentials already exist at '{}'; reusing existing account",
            cred_path.display()
        );

        let bytes = tokio::fs::read(&cred_path).await.with_context(|| {
            format!(
                "failed reading existing acme-dns credentials at {}",
                cred_path.display()
            )
        })?;

        let creds: Credentials = serde_json::from_slice(&bytes)
            .context("failed to parse existing acme-dns credentials as JSON")?;

        return Ok((creds, false));
    }

    // No existing creds → perform a fresh registration.
    let client = AcmeDnsClient::new(api_base)
        .with_context(|| format!("failed to construct AcmeDnsClient for {api_base}"))?;

    // `allow_from` is already `Option<&[String]>`, pass straight through.
    let creds: Credentials = client
        .register(allow_from)
        .await
        .context("acme-dns registration failed")?;

    // Persist credentials for next run.
    let json = serde_json::to_vec_pretty(&creds)
        .context("failed to serialize acme-dns credentials to JSON")?;
    tokio::fs::write(&cred_path, &json).await.with_context(|| {
        format!(
            "failed to write acme-dns credentials to {}",
            cred_path.display()
        )
    })?;

    Ok((creds, true))
}
