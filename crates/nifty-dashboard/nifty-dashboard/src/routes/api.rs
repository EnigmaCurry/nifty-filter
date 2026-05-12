use aide::axum::ApiRouter;

use super::{config, dnsmasq, healthz, hello, qos, services, status, updates, whoami};
use crate::prelude::*;

pub fn router(state: AppState) -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .nest("/config", config::router())
        .nest("/dnsmasq", dnsmasq::router())
        .nest("/healthz", healthz::router())
        .nest("/hello", hello::router(state))
        .nest("/qos", qos::router())
        .nest("/services", services::router())
        .nest("/status", status::router())
        .nest("/updates", updates::router())
        .nest("/whoami", whoami::router())
}
