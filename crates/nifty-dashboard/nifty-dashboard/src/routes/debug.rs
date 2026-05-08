use crate::prelude::*;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use axum::{routing::get, Router};
use rmp_serde::from_slice;
use serde::Serialize;
use tower_sessions::session::Record; // or `tower_sessions_core::session::Record`

/// All routes that live under `/debug`.
pub fn router() -> Router<AppState> {
    Router::<AppState>::new().route("/list_sessions", get(debug_list_sessions))
}

/// Change this if you've customized the table name with `with_table_name`.
const SESSIONS_TABLE: &str = "tower_sessions";

#[derive(Serialize)]
struct DebugRecord {
    #[serde(flatten)]
    inner: Record,
}

pub async fn debug_list_sessions(State(state): State<AppState>) -> impl IntoResponse {
    let query = format!("select data from {table}", table = SESSIONS_TABLE,);

    let rows: Vec<(Vec<u8>,)> = match sqlx::query_as(&query).fetch_all(&state.db).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("failed to fetch sessions: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut out = Vec::with_capacity(rows.len());
    for (blob,) in rows {
        let record: Record = match from_slice(&blob) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("failed to decode session: {e}");
                continue;
            }
        };

        out.push(DebugRecord { inner: record });
    }

    Json(out).into_response()
}
