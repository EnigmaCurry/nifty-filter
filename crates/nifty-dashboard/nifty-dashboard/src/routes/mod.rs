use aide::{axum::ApiRouter, openapi::OpenApi};
use axum::response::IntoResponse;
use axum::{
    Extension, error_handling::HandleErrorLayer, http::StatusCode, middleware,
    routing::get,
};
use axum_oidc::{EmptyAdditionalClaims, OidcAuthLayer, error::MiddlewareError};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{services::ServeDir, trace::TraceLayer};

use crate::middleware::require_role::RequireRoles;
use crate::models::role::SystemRole;
use crate::{
    AppState,
    api_docs::{configure_openapi, docs_routes},
    middleware::{
        csrf_protection, oidc, require_role::require_roles_middleware, trusted_forwarded_for,
        trusted_header_auth, user_session::user_session_middleware,
    },
};

pub mod admin;
pub mod api;
pub mod config;
pub mod events;
pub mod healthz;
pub mod hello;
pub mod login;
pub mod qos;
pub mod status;
pub mod updates;
pub mod whoami;

pub fn router(
    forward_auth_cfg: trusted_header_auth::ForwardAuthConfig,
    forwarded_for_cfg: trusted_forwarded_for::TrustedForwardedForConfig,
    oidc_cfg: oidc::OidcConfig,
    oidc_auth_layer: Option<OidcAuthLayer<EmptyAdditionalClaims>>,
    state: AppState,
) -> axum::Router<AppState> {
    // build your ApiRouter pieces as you already do...
    let public_api = ApiRouter::<AppState>::new()
        .nest("/api", api::router(state.clone()))
        .layer(middleware::from_fn(csrf_protection::csrf_middleware));

    let login_api = login::router(forward_auth_cfg)
        .layer(middleware::from_fn(csrf_protection::csrf_middleware));

    let admin_api = ApiRouter::<AppState>::new()
        .nest("/admin", admin::router())
        .layer(middleware::from_fn_with_state(
            (state.clone(), RequireRoles(&[SystemRole::Admin])),
            require_roles_middleware,
        ))
        .layer(middleware::from_fn(csrf_protection::csrf_middleware));

    let mut api_spec = OpenApi::default();

    // Finish the ApiRouter FIRST (so aide can collect OpenAPI info)
    let api_router: ApiRouter<AppState> = ApiRouter::<AppState>::new()
        .merge(public_api)
        .merge(login_api)
        .nest_api_service("/docs", docs_routes())
        .merge(admin_api)
        .finish_api_with(&mut api_spec, configure_openapi)
        .layer(Extension(Arc::new(api_spec)))
        .nest_service("/static", ServeDir::new("static"))
        .route("/favicon.ico", get(favicon))
        .into();

    // Convert to axum::Router so we can add fallible layers (OIDC)
    let mut app: axum::Router<AppState> = api_router.into();

    if oidc_cfg.enabled {
        let oidc_auth_layer =
            oidc_auth_layer.expect("oidc_cfg.enabled == true but oidc_auth_layer is None");

        app = app.layer(
            ServiceBuilder::new()
                // OUTER: catches MiddlewareError from inner layers and returns a Response
                .layer(HandleErrorLayer::<_, ()>::new(
                    |e: MiddlewareError| async move { e.into_response() },
                ))
                // INNER: may produce MiddlewareError
                .layer(oidc_auth_layer),
        );
    }

    // Global middleware (these are fine on Router too)
    app.layer(middleware::from_fn(user_session_middleware))
        .layer(middleware::from_fn_with_state(
            forwarded_for_cfg,
            trusted_forwarded_for::trusted_forwarded_for,
        ))
        .route("/api/events", get(events::sse_handler))
        .route("/", get(crate::frontend::spa_handler))
        .route("/{*path}", get(crate::frontend::spa_handler))
        .layer(TraceLayer::new_for_http())
}

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}
