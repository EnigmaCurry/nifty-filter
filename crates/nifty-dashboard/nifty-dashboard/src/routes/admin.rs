use std::collections::HashMap;

use crate::errors::ErrorBody;
use crate::models::user::{self, PublicUser};
use crate::prelude::*;
use crate::response::{ApiJson, ApiResponse, json_error, json_ok};
use aide::{NoApi, axum::ApiRouter};
use api_doc_macros::{api_doc, get_with_docs};
use axum::extract::Path;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use indexmap::IndexMap;
use rmp_serde::from_slice;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use time::OffsetDateTime;
use tower_sessions::session::Record;

pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .api_route("/list_sessions", get_with_docs!(list_sessions))
        .api_route("/user/{user_id}", get_with_docs!(get_user))
}

const SESSIONS_TABLE: &str = "tower_sessions";

#[derive(Serialize, JsonSchema)]
struct SessionRecord {
    data: HashMap<String, JsonValue>,
    /// Number of seconds until this session expires (clamped at 0).
    validity_seconds: i64,
}

impl SessionRecord {
    fn from_record(r: Record, now: OffsetDateTime) -> Option<Self> {
        let secs_left = (r.expiry_date - now).whole_seconds();

        if secs_left <= 0 {
            return None;
        }

        Some(SessionRecord {
            data: r.data,
            validity_seconds: secs_left,
        })
    }
}

#[derive(Deserialize, JsonSchema)]
struct ListSessionsQuery {
    /// Zero-based offset into the session list.
    #[serde(default)]
    offset: u32,
    /// Maximum number of sessions to return.
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    100
}

const MAX_LIMIT: u32 = 500;

#[derive(Serialize, JsonSchema)]
pub struct ListSessionsResponse {
    /// Map of session id -> session record.
    items: IndexMap<String, SessionRecord>,
    /// Offset used for this page.
    offset: u32,
    /// Limit used for this page.
    limit: u32,
    /// Offset to request the next page, if any.
    next_offset: Option<u32>,
}

#[api_doc(
    id = "admin_list_sessions",
    tag = "admin",
    ok = "Json<ApiResponse<ListSessionsResponse>>",
    err = "Json<ApiResponse<()>>"
)]
/// List active sessions.
///
/// Supports offset/limit pagination. Returns an ordered map of active session IDs
/// to their session data and remaining validity time (in seconds).
async fn list_sessions(
    NoApi(State(state)): NoApi<State<AppState>>,
    NoApi(Query(params)): NoApi<Query<ListSessionsQuery>>,
) -> ApiJson<ListSessionsResponse> {
    let offset = params.offset;
    let limit = params.limit.min(MAX_LIMIT);
    let fetch_limit = limit.saturating_add(1); // fetch one extra to detect "has more"

    let query = format!(
        "select id, data from {table} order by expiry_date asc limit ? offset ?",
        table = SESSIONS_TABLE,
    );

    let rows: Vec<(String, Vec<u8>)> = match sqlx::query_as(&query)
        .bind(fetch_limit as i64)
        .bind(offset as i64)
        .fetch_all(&state.db)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("failed to fetch sessions: {e}");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error");
        }
    };

    let has_more = rows.len() as u32 > limit;
    let next_offset = if has_more {
        Some(offset.saturating_add(limit))
    } else {
        None
    };

    let now = OffsetDateTime::now_utc();
    let mut items: IndexMap<String, SessionRecord> = IndexMap::with_capacity(limit as usize);

    // Only keep up to `limit` rows in this page
    for (id_str, blob) in rows.into_iter().take(limit as usize) {
        let record: Record = match from_slice(&blob) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("failed to decode session {id_str}: {e}");
                continue;
            }
        };

        if let Some(session) = SessionRecord::from_record(record, now) {
            items.insert(id_str, session);
        }
    }

    let payload = ListSessionsResponse {
        items,
        offset,
        limit,
        next_offset,
    };

    json_ok(payload)
}

/// The shape of the JSON we send back from User API.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UserResponse {
    user: PublicUser,
}

#[api_doc(
    id = "user",
    tag = "admin",
    ok = "Json<ApiResponse<UserResponse>>",
    err = "Json<ErrorBody>"
)]
pub async fn get_user(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> ApiJson<UserResponse> {
    let maybe_user = user::select_user_by_username(&state.db, &username)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("database query error: {e}"),
            )
        })
        .unwrap();

    match maybe_user {
        Some(user) => {
            let username = user.username.unwrap_or("".to_string());
            if username.is_empty() {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, "database error")
            } else {
                json_ok(UserResponse {
                    user: PublicUser { username },
                })
            }
        }
        None => json_error(StatusCode::NOT_FOUND, "user not found"),
    }
}
