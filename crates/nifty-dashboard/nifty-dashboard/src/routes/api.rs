use aide::axum::ApiRouter;

use super::{config, healthz, hello, qos, status, updates, whoami};
use crate::prelude::*;

pub fn router(state: AppState) -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .nest("/config", config::router())
        .nest("/healthz", healthz::router())
        .nest("/hello", hello::router(state))
        .nest("/qos", qos::router())
        .nest("/status", status::router())
        .nest("/updates", updates::router())
        .nest("/whoami", whoami::router())
}
