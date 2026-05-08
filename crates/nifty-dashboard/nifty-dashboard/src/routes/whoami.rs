use std::collections::BTreeMap;

use aide::{NoApi, axum::ApiRouter};
use api_doc_macros::{api_doc, get_with_docs};
use axum::{
    Json,
    extract::{OriginalUri, Request},
    http::header::{HOST, USER_AGENT},
};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    errors::ErrorBody,
    middleware::user_session::UserSession,
    prelude::*,
    response::{ApiJson, ApiResponse, json_ok},
};

/// All routes that live under `/whoami`.
pub fn router() -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new().api_route("/", get_with_docs!(whoami_json))
}

/// The subset of session data we want to expose publicly from Whoami API.
#[derive(Debug, Serialize, JsonSchema)]
struct WhoamiSessionData {
    #[serde(default)]
    pub is_logged_in: bool,
    pub username: Option<String>,
    pub external_user_id: Option<String>,
    pub client_ip: Option<String>,
    pub csrf_token: String,
    pub visit_count: u64,
}

/// The shape of the JSON we send back from Whoami API.
#[derive(Debug, Serialize, JsonSchema)]
struct WhoamiResponse {
    request: BTreeMap<String, String>,
    session: WhoamiSessionData,
}

#[api_doc(
    id = "whoami",
    tag = "session",
    ok = "Json<ApiResponse<WhoamiResponse>>",
    err = "Json<ErrorBody>"
)]
/// Get session data
///
/// Returns request metadata and a subset of the current user's session.
async fn whoami_json(
    NoApi(user_session): NoApi<UserSession>,
    NoApi(original_uri): NoApi<OriginalUri>,
    NoApi(req): NoApi<Request>,
) -> ApiJson<WhoamiResponse> {
    let mut req_map: BTreeMap<String, String> = BTreeMap::new();

    req_map.insert("path".to_string(), original_uri.0.path().to_string());
    req_map.insert("method".to_string(), req.method().as_str().to_string());

    let headers = req.headers();
    for (name, value) in headers.iter() {
        if name != HOST && name != USER_AGENT {
            continue;
        }
        let val_str = value.to_str().unwrap_or("<non-utf8>");
        req_map.insert(name.as_str().to_string(), val_str.to_string());
    }

    let session = WhoamiSessionData {
        client_ip: match &user_session.client_ip {
            Some(ip) => Some(ip.clone()),
            None => Some(user_session.peer_ip.clone()),
        },
        username: user_session.username,
        external_user_id: user_session.external_user_id,
        is_logged_in: user_session.is_logged_in,
        visit_count: user_session.visit_count,
        csrf_token: user_session.csrf_token.clone(),
    };

    let payload = WhoamiResponse {
        request: req_map,
        session,
    };

    json_ok(payload)
}
