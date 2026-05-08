use crate::{
    AppState, errors::AppError, middleware::user_session::UserSession, models::role::SystemRole,
};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::IntoResponse,
};
use tracing::warn;

#[derive(Clone, Copy)]
pub struct RequireRoles(pub &'static [SystemRole]);

pub async fn require_roles_middleware(
    State((state, required)): State<(AppState, RequireRoles)>,
    user_session: UserSession,
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, AppError> {
    let path = request.uri().path().to_string();

    // 1) Logged in?
    if !user_session.is_logged_in {
        warn!(
            %path,
            username = user_session.username,
            client_ip = user_session.client_ip.as_deref().unwrap_or("<unknown>"),
            "role access denied: not logged in",
        );
        return Err(AppError::unauthorized("Not logged in"));
    }

    let user_id = user_session.user_id;
    let roles = required.0;

    // empty requirement means "allow" (choose this or treat as bug)
    if roles.is_empty() {
        return Ok(next.run(request).await);
    }

    // 2) Has any required role?
    //
    // Build:  ... AND r.name IN (?2, ?3, ?4 ...)
    // (we bind user_id at ?1, so roles start at ?2)
    let mut sql = String::from(
        r#"
        SELECT 1
        FROM user_role ur
        JOIN [role] r ON r.id = ur.role_id
        WHERE ur.user_id = ?1
          AND r.name IN ("#,
    );

    for i in 0..roles.len() {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push('?');
        sql.push_str(&(i + 2).to_string());
    }
    sql.push_str(")\nLIMIT 1\n");

    let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(&user_id);
    for r in roles {
        q = q.bind(r.as_ref());
    }

    let has_role = q
        .fetch_optional(&state.db)
        .await
        .map_err(|e| AppError::internal(&format!("failed to check roles: {e}")))?
        .is_some();

    if !has_role {
        warn!(
            %path,
            ?user_id,
            username = user_session.username,
            client_ip = user_session.client_ip.as_deref().unwrap_or("<unknown>"),
            required_roles = ?roles,
            "role access denied: user lacks required role",
        );
        return Err(AppError::forbidden("required role missing"));
    }

    // 3) Continue
    Ok(next.run(request).await)
}
