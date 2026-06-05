use std::collections::HashSet;
use std::path::Path;

use log::{info, error, warn};

use crate::config::{TraefikConfig, RouteConfig};

/// Write Traefik dynamic config files for all declared routes.
/// Routes without `allow_from` are skipped (no open access by default).
/// Stale route files from previous configs are cleaned up.
pub fn write_routes(dir: &Path, domain: &str, traefik: Option<&TraefikConfig>) {
    let routes = traefik.map(|t| &t.route);
    let declared: HashSet<String> = routes
        .map(|r| r.keys().cloned().collect())
        .unwrap_or_default();

    // Write config for each declared route.
    if let Some(routes) = routes {
        for (name, route) in routes {
            write_route(dir, domain, name, route);
        }
    }

    // Clean up stale route files from previous configs.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|f| f.to_str()) {
                Some(f) => f.to_string(),
                None => continue,
            };
            // Only manage route-*.yml files; leave tls.yml etc. alone.
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

    let config = serde_json::json!({
        "http": {
            "routers": {
                name: {
                    "rule": rule,
                    "service": name,
                    "entryPoints": ["websecure"],
                    "tls": {
                        "certResolver": "step-ca"
                    }
                }
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
            info!("traefik: wrote route '{name}' -> {host} (allow: {})", route.allow_from.join(", "));
        }
    }
}
