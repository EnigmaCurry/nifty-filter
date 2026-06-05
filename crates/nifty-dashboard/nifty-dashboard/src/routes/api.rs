use aide::axum::ApiRouter;

use super::{config, ddns, dnsmasq, healthz, hello, mdns, qos, services, status, technitium, updates, whoami};
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
        .nest("/status", status::router())
        .nest("/technitium", technitium::router())
        .nest("/updates", updates::router())
        .nest("/whoami", whoami::router())
}
