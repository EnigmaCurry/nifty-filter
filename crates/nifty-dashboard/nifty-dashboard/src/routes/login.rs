use crate::errors::AppError;
use crate::middleware::auth::AuthenticationMethod;
use crate::models::identity_provider::IdentityProviders;
use crate::models::user_status::UserStatus;
use crate::response::{ApiJson, ApiResponse, json_empty_ok, json_error};
use crate::{
    middleware::{
        trusted_header_auth, trusted_header_auth::ForwardAuthUser, user_session::UserSession,
    },
    models::user,
    prelude::*,
    server::AppState,
};
use aide::axum::IntoApiResponse;
use aide::{NoApi, axum::ApiRouter};
use api_doc_macros::{api_doc, post_with_docs};
use axum::error_handling::HandleErrorLayer;
use axum::routing::{get, post};
use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Redirect},
};
use axum_oidc::error::MiddlewareError;
use axum_oidc::{EmptyAdditionalClaims, OidcClaims, OidcLoginLayer};
use schemars::JsonSchema;
use serde::Deserialize;
use tower::ServiceBuilder;
use tower_sessions::Session;

pub fn router(user_cfg: trusted_header_auth::ForwardAuthConfig) -> ApiRouter<AppState> {
    match user_cfg.method {
        AuthenticationMethod::ForwardAuth => trusted_header_router(user_cfg),
        AuthenticationMethod::UsernamePassword => username_password_router(),
        AuthenticationMethod::Oidc => oidc_router(),
    }
}

/// Request body for username/password login.
#[derive(Deserialize, JsonSchema)]
struct PasswordLoginRequest {
    /// The username for this account.
    username: String,
    /// The plaintext password for this account.
    password: String,
}

fn trusted_header_router(user_cfg: trusted_header_auth::ForwardAuthConfig) -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .api_route(
            "/api/login",
            post_with_docs!(forward_auth_login_handler).layer(middleware::from_fn_with_state(
                user_cfg,
                trusted_header_auth::trusted_header_auth,
            )),
        )
        .api_route("/api/logout", post_with_docs!(logout_handler))
}

fn username_password_router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .api_route(
            "/api/login",
            post_with_docs!(username_password_login_handler),
        )
        .api_route("/api/logout", post_with_docs!(logout_handler))
}

fn oidc_router() -> ApiRouter<AppState> {
    let oidc_login_service = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|e: MiddlewareError| async {
            dbg!(&e);
            e.into_response()
        }))
        .layer(OidcLoginLayer::<EmptyAdditionalClaims>::new());

    ApiRouter::<AppState>::new()
        .route("/api/login", get(oidc_login_handler))
        .route("/api/login", post(oidc_login_handler))
        .api_route("/api/logout", post_with_docs!(logout_handler))
        .layer(oidc_login_service)
}

#[api_doc(
    id = "login_forward_auth",
    tag = "session",
    ok = "Json<ApiResponse<()>>",
    err = "Json<ApiResponse<()>>"
)]
/// Log in using a trusted authentication header.
///
/// This endpoint expects a reverse proxy (e.g. Traefik) to provide the
/// authenticated user via headers. On success, the user session is updated.
async fn forward_auth_login_handler(
    State(state): State<AppState>,
    NoApi(Extension(trusted_user)): NoApi<Extension<ForwardAuthUser>>,
    NoApi(mut user_session): NoApi<UserSession>,
    NoApi(session): NoApi<Session>,
) -> ApiJson<()> {
    let external_id = trusted_user.external_id.clone();
    tracing::debug!("POST /api/login (forward auth) external_id = {external_id}");

    let user = match user::get_or_create_by_external_id(
        &state.db,
        &external_id,
        IdentityProviders::ForwardAuth,
    )
    .await
    {
        Ok(user) => user,
        Err(err) => {
            tracing::error!(
                "Error in forward_auth_login_handler get_or_create_by_external_id: {err}"
            );
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error");
        }
    };

    match finish_login_for_user(user, &mut user_session, &session).await {
        Ok(()) => json_empty_ok(),
        Err(err) => map_login_error(err),
    }
}

#[api_doc(
    id = "login_password",
    tag = "session",
    ok = "Json<ApiResponse<()>>",
    err = "Json<ApiResponse<()>>"
)]
/// Log in with email and password.
///
/// Returns 200 on success, or 401/403 for invalid credentials or disabled accounts.
async fn username_password_login_handler(
    State(state): State<AppState>,
    NoApi(mut user_session): NoApi<UserSession>,
    NoApi(session): NoApi<Session>,
    Json(body): Json<PasswordLoginRequest>,
) -> ApiJson<()> {
    const INVALID_CREDENTIALS: &str = "invalid username or password";

    let user = match user::select_user_by_username(&state.db, &body.username).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return json_error(StatusCode::UNAUTHORIZED, INVALID_CREDENTIALS);
        }
        Err(err) => {
            tracing::error!("Error in username_password_login_handler select_user_by_email: {err}");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error");
        }
    };

    match user.verify_password(&state.db, &body.password).await {
        Ok(true) => { /* continue */ }
        Ok(false) => {
            return json_error(StatusCode::UNAUTHORIZED, INVALID_CREDENTIALS);
        }
        Err(err) => {
            tracing::error!("Error in username_password_login_handler verify_password: {err}");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error");
        }
    }

    match finish_login_for_user(user, &mut user_session, &session).await {
        Ok(()) => json_empty_ok(),
        Err(err) => map_login_error(err),
    }
}

/// Internal helper that mutates `UserSession` and persists it.
///
/// Returns domain-level errors as `AppError` which we map into JSON in the
/// handlers above.
async fn finish_login_for_user(
    user: user::User,
    user_session: &mut UserSession,
    session: &Session,
) -> AppResult<()> {
    if user.status != UserStatus::Active {
        return Err(AppError::forbidden("your account has been disabled"));
    }

    user_session.user_id = user.id.0;
    user_session.external_user_id = Some(user.external_id.clone());
    user_session.username = user.username;
    user_session.is_logged_in = true;

    user_session.persist(session).await?;

    Ok(())
}

/// Common mapping from `AppError` to a JSON API response for this API.
fn map_login_error(err: AppError) -> ApiJson<()> {
    let status = err.status;
    tracing::warn!("login error (status={status}): {err:?}");

    let msg = match status {
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        _ => "internal server error",
    };

    json_error(status, msg)
}

#[api_doc(
    id = "logout",
    tag = "session",
    ok = "Json<ApiResponse<()>>",
    err = "Json<ApiResponse<()>>"
)]
/// Log out the current user.
///
/// Clears the session and returns 200 on success.
async fn logout_handler(
    NoApi(mut user_session): NoApi<UserSession>,
    NoApi(session): NoApi<Session>,
) -> ApiJson<()> {
    user_session.external_user_id = None;
    user_session.is_logged_in = false;

    if let Err(err) = session.flush().await {
        tracing::error!("Failed to flush session on logout: {err}");
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error");
    }

    json_empty_ok()
}

#[derive(Deserialize, JsonSchema)]
pub struct LoginQuery {
    next: Option<String>,
}

fn safe_next(next: Option<String>) -> &'static str {
    // Minimal safe default; you can implement proper “relative-path only” validation.
    // Avoid open redirects.
    match next.as_deref() {
        Some(p) if p.starts_with('/') && !p.starts_with("//") => "/",
        _ => "/",
    }
}

pub async fn oidc_login_handler(
    State(state): State<AppState>,
    mut user_session: UserSession,
    session: Session,
    Query(q): Query<LoginQuery>,
    claims: OidcClaims<EmptyAdditionalClaims>,
) -> impl IntoApiResponse {
    if let Some(external_id) = claims.email() {
        debug!("Logging in external OIDC user: {external_id:?}");
        let user = match user::get_or_create_by_external_id(
            &state.db,
            &external_id,
            IdentityProviders::Oidc,
        )
        .await
        {
            Ok(user) => user,
            Err(err) => {
                tracing::error!("oidc_login_handler get_or_create_by_external_id: {err}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                    .into_response();
            }
        };

        if let Err(err) = finish_login_for_user(user, &mut user_session, &session).await {
            return map_login_error(err).into_response();
        }
        Redirect::to(safe_next(q.next)).into_response()
    } else {
        tracing::error!(
            "oidc_login_handler get_or_create_by_external_id: User did not have an external_id"
        );
        return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
    }
}
