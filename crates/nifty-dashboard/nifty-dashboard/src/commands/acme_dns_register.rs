use std::io::Write;

use crate::{
    config::AcmeDnsRegisterConfig, ensure_root_dir, errors::CliError,
    tls::dns::register_acme_dns_account, util::write_files::create_private_dir_all_0700_sync,
};
use anyhow::Context;

pub fn acme_dns_register<W1: Write, W2: Write>(
    args: AcmeDnsRegisterConfig,
    root_dir: std::path::PathBuf,
    out: &mut W1,
    _err: &mut W2,
) -> Result<(), CliError> {
    let root_dir = ensure_root_dir(root_dir)?;
    // Where to store creds:
    let cache_dir = root_dir.join("tls-cache");

    create_private_dir_all_0700_sync(&cache_dir)
        .context(format!("TLS cache dir invalid: {}", cache_dir.display()))?;

    // Build domain list from NET_HOST + TLS_SANS for CNAME hints
    let mut domains: Vec<String> = Vec::new();

    if let Some(ref host) = args.host
        && !host.trim().is_empty()
    {
        domains.push(host.clone());
    }

    for s in &args.sans.0 {
        if !s.trim().is_empty() {
            domains.push(s.clone());
        }
    }

    // Dedup
    let mut seen = std::collections::BTreeSet::new();
    domains.retain(|d| seen.insert(d.clone()));

    // Build allow_from
    let allow_from_opt = if args.allow_from.is_empty() {
        None
    } else {
        Some(args.allow_from.clone())
    };

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| CliError::RuntimeError(format!("Failed to start Tokio runtime: {e}")))?;

    let (creds, created_new) = rt
        .block_on(register_acme_dns_account(
            &args.api_base,
            &cache_dir,
            &domains,
            allow_from_opt.as_deref(),
        ))
        .map_err(|e| CliError::RuntimeError(e.to_string()))?;

    let cred_path = cache_dir.join("acme-dns-credentials.json");

    if created_new {
        writeln!(
            out,
            "Registered new acme-dns account and wrote credentials to:\n  {}\n",
            cred_path.display()
        )?;
    } else {
        writeln!(
            out,
            "Using existing acme-dns account credentials from:\n  {}\n",
            cred_path.display()
        )?;
    }

    writeln!(out, "acme-dns fulldomain:\n  {}", creds.fulldomain)?;

    let cname_help = format_acme_dns_cname_help(&domains, &creds.fulldomain);
    write!(out, "{cname_help}")?;

    Ok(())
}

/// Build a human-readable help block telling the user which CNAMEs to create
/// for ACME DNS-01, given the domains their app serves and the acme-dns
/// `fulldomain` value.
///
/// `domains` should be the same set you pass to the ACME TLS layer
/// (derived from NET_HOST + TLS_SANS).
fn format_acme_dns_cname_help(domains: &[String], fulldomain: &str) -> String {
    // No domain list? Fall back to a generic example.
    if domains.is_empty() {
        return format!(
            "\nConfigure your public DNS with a CNAME like:\n  \
             _acme-challenge.<your-domain> IN CNAME {fulldomain}\n"
        );
    }

    let mut out = String::from("\n\nFor each domain, configure a CNAME in your public DNS:\n");
    for d in domains {
        if d.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("  _acme-challenge.{d} IN CNAME {fulldomain}\n"));
    }
    out.push_str(
        "\nPlease ensure these DNS records exist before running `serve`.\nUse `dig` to verify:\n",
    );
    for d in domains {
        if d.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("\ndig +short CNAME _acme-challenge.{d} @1.1.1.1"));
    }
    out.push('\n');
    out
}
