use aide::axum::ApiRouter;

use super::{config, healthz, hello, status, whoami};
use crate::prelude::*;

pub fn router(state: AppState) -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .nest("/config", config::router())
        .nest("/healthz", healthz::router())
        .nest("/hello", hello::router(state))
        .nest("/status", status::router())
        .nest("/whoami", whoami::router())
}
