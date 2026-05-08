use aide::{NoApi, axum::ApiRouter};
use api_doc_macros::{api_doc, get_with_docs, post_with_docs};
use axum::{Json, http::StatusCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    errors::ErrorBody,
    middleware::user_session::UserSession,
    response::{ApiJson, ApiResponse, json_error, json_ok},
};

pub fn router(_state: AppState) -> ApiRouter<AppState> {
    ApiRouter::<AppState>::new()
        .api_route("/", post_with_docs!(formal_greeting))
        .api_route("/", get_with_docs!(hello))
}

#[derive(Deserialize, JsonSchema)]
struct HelloRequest {
    /// Name to greet.
    name: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct Greeting {
    message: String,
}

#[api_doc(
    id = "hello",
    tag = "example",
    ok = "Json<ApiResponse<Greeting>>",
    err = "Json<ErrorBody>"
)]
/// Say hello
///
/// Returns a public greeting message to the logged in user or to the world at large.
async fn hello(NoApi(user_session): NoApi<UserSession>) -> ApiJson<Greeting> {
    if let Some(external_user_id) = user_session.external_user_id {
        json_ok(Greeting {
            message: format!("Hello, {external_user_id}!"),
        })
    } else {
        json_ok(Greeting {
            message: format!("Hello World!"),
        })
    }
}

#[api_doc(
    id = "formal_greeting",
    tag = "example",
    ok = "Json<ApiResponse<Greeting>>",
    err = "Json<ErrorBody>"
)]
/// Say hello
///
/// Returns a greeting message, optionally personalized with the requested name, but only for logged in users.
async fn formal_greeting(
    NoApi(user_session): NoApi<UserSession>,
    Json(body): Json<HelloRequest>,
) -> ApiJson<Greeting> {
    if !user_session.is_logged_in {
        return json_error(
            StatusCode::FORBIDDEN,
            "Sorry, you must be logged in to POST to this endpoint. Public requests may GET instead.",
        );
    }
    let name = body.name.unwrap_or_else(|| "world".to_string());
    json_ok(Greeting {
        message: format!("Greetings, {name}!"),
    })
}
