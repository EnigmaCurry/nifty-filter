use std::collections::HashSet;
use std::path::Path;

use log::{info, error, warn};

use crate::config::{TraefikConfig, RouteConfig};

/// Write Traefik dynamic config files for all declared routes.
/// Routes without `allow_from` are skipped (no open access by default).
/// Stale route files from previous configs are cleaned up.
/// Also writes a shared mTLS TLS options file if any route uses `authorized_clients`.
pub fn write_routes(dir: &Path, domain: &str, traefik: Option<&TraefikConfig>) {
    let routes = traefik.map(|t| &t.route);
    let declared: HashSet<String> = routes
        .map(|r| r.keys().cloned().collect())
        .unwrap_or_default();

    let mut any_mtls = false;

    // Write config for each declared route.
    if let Some(routes) = routes {
        for (name, route) in routes {
            if !route.authorized_clients.is_empty() {
                any_mtls = true;
            }
            write_route(dir, domain, name, route);
        }
    }

    // Write or remove the shared mTLS TLS options file.
    write_mtls_tls_options(dir, any_mtls);

    // Clean up stale route files from previous configs.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|f| f.to_str()) {
                Some(f) => f.to_string(),
                None => continue,
            };
            // Only manage route-*.yml files; leave tls.yml, mtls-options.yml etc. alone.
            if let Some(route_name) = file_name.strip_prefix("route-").and_then(|s| s.strip_suffix(".yml")) {
                if !declared.contains(route_name) {
                    info!("traefik: removing stale route file {file_name}");
                    if let Err(e) = std::fs::remove_file(&path) {
                        error!("traefik: failed to remove {}: {e}", path.display());
                    }
                }
            }
        }
    }
}

fn write_route(dir: &Path, domain: &str, name: &str, route: &RouteConfig) {
    if route.allow_from.is_empty() {
        warn!("traefik: route '{name}' has no allow_from — skipping (no open access by default)");
        // Remove the file if it exists from a previous config.
        let path = dir.join(format!("route-{name}.yml"));
        let _ = std::fs::remove_file(&path);
        return;
    }

    let host = format!("{name}.{domain}");

    // Build Traefik rule: Host(`...`) && (ClientIP(`...`) || ClientIP(`...`))
    let client_ip_clauses: Vec<String> = route
        .allow_from
        .iter()
        .map(|cidr| format!("ClientIP(`{cidr}`)"))
        .collect();
    let client_ip = client_ip_clauses.join(" || ");
    let rule = format!("Host(`{host}`) && ({client_ip})");

    let use_mtls = !route.authorized_clients.is_empty();

    let mut tls_config = serde_json::json!({
        "certResolver": "step-ca"
    });
    if use_mtls {
        tls_config["options"] = serde_json::json!("mtls");
    }

    let mut router = serde_json::json!({
        "rule": rule,
        "service": name,
        "entryPoints": ["websecure"],
        "tls": tls_config
    });

    let mut config = serde_json::json!({
        "http": {
            "routers": {
                name: router
            },
            "services": {
                name: {
                    "loadBalancer": {
                        "servers": [
                            { "url": &route.backend }
                        ]
                    }
                }
            }
        }
    });

    // Add certauthz middleware when authorized_clients is configured.
    if use_mtls {
        let middleware_name = format!("{name}-certauthz");
        router["middlewares"] = serde_json::json!([&middleware_name]);
        // Update router in config
        config["http"]["routers"][name] = router;
        config["http"]["middlewares"] = serde_json::json!({
            &middleware_name: {
                "plugin": {
                    "certauthz": {
                        "domains": &route.authorized_clients
                    }
                }
            }
        });
    }

    let path = dir.join(format!("route-{name}.yml"));
    let content = serde_json::to_string_pretty(&config).unwrap();

    // Only write if content changed to avoid unnecessary Traefik reloads.
    let needs_write = match std::fs::read_to_string(&path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };

    if needs_write {
        if let Err(e) = std::fs::write(&path, &content) {
            error!("traefik: failed to write {}: {e}", path.display());
        } else {
            let auth_info = if use_mtls {
                format!(", authorized_clients: {}", route.authorized_clients.join(", "))
            } else {
                String::new()
            };
            info!("traefik: wrote route '{name}' -> {host} (allow: {}{auth_info})", route.allow_from.join(", "));
        }
    }
}

/// Write a shared TLS options file that defines the "mtls" option requiring
/// client certificates verified against the Step-CA root.
fn write_mtls_tls_options(dir: &Path, needed: bool) {
    let path = dir.join("mtls-options.yml");
    if !needed {
        let _ = std::fs::remove_file(&path);
        return;
    }

    let config = serde_json::json!({
        "tls": {
            "options": {
                "mtls": {
                    "clientAuth": {
                        "caFiles": ["/etc/ssl/step-ca-root.crt"],
                        "clientAuthType": "RequireAndVerifyClientCert"
                    }
                }
            }
        }
    });

    let content = serde_json::to_string_pretty(&config).unwrap();
    let needs_write = match std::fs::read_to_string(&path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };

    if needs_write {
        if let Err(e) = std::fs::write(&path, &content) {
            error!("traefik: failed to write mtls-options.yml: {e}");
        } else {
            info!("traefik: wrote mtls-options.yml (RequireAndVerifyClientCert)");
        }
    }
}
