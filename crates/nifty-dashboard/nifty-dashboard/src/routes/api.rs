use aide::axum::ApiRouter;
use axum::middleware;

use super::{config, ddns, dnsmasq, healthz, hello, mdns, qos, services, services_config, status, technitium, updates, whoami};
use crate::middleware::require_subnet;
use crate::prelude::*;

pub fn router(state: AppState) -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .nest("/config", config::router())
        .nest("/ddns", ddns::router())
        .nest("/dnsmasq", dnsmasq::router())
        .nest("/healthz", healthz::router())
        .nest("/mdns", mdns::router())
        .nest("/hello", hello::router(state))
        .nest("/qos", qos::router())
        .nest("/services", services::router())
        .nest(
            "/services-config",
            services_config::router()
                .layer(middleware::from_fn(require_subnet::require_subnet)),
        )
        .nest("/status", status::router())
        .nest("/technitium", technitium::router())
        .nest("/updates", updates::router())
        .nest("/whoami", whoami::router())
}
