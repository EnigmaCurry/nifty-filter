use crate::middleware::user_session::UserSession;
use axum::{
    body::Body,
    extract::Path,
    http::{Response, StatusCode},
    response::{IntoResponse, Redirect},
};
use rust_embed::RustEmbed;
use tracing::error;

const CSRF_PLACEHOLDER: &str = "__CSRF_TOKEN_REPLACED_HERE_AT_RUNTIME__";

#[derive(RustEmbed)]
#[folder = "../frontend/build"]
struct Frontend;

pub async fn spa_handler(
    maybe_path: Option<Path<String>>,
    user_session: UserSession,
) -> impl IntoResponse {
    let requested: String = maybe_path.map(|Path(p)| p).unwrap_or_default();

    let requested = requested.trim_matches('/').to_string();

    // ---------- 0) Normalize *.html paths ----------
    if let Some(stripped) = requested.strip_suffix(".html") {
        // /index.html -> /
        let target = if stripped.is_empty() || stripped == "index" {
            "/".to_string()
        } else {
            format!("/{}", stripped)
        };

        // IMPORTANT: convert Redirect into a Response so all branches
        // return the same concrete type.
        return Redirect::permanent(&target).into_response();
    }

    // ---------- 1) Static assets ----------
    let asset_path: &str = if let Some(idx) = requested.find("_app/") {
        &requested[idx..]
    } else if !requested.is_empty() {
        &requested
    } else {
        ""
    };

    if !asset_path.is_empty()
        && let Some(content) = Frontend::get(asset_path)
    {
        let mime = mime_guess::from_path(asset_path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", mime.as_ref())
            .body(Body::from(content.data))
            .unwrap();
    }

    // ---------- 2) HTML routes ----------
    let candidates: Vec<String> = if requested.is_empty() {
        vec!["index.html".to_string()]
    } else {
        vec![
            format!("{requested}.html"),
            format!("{requested}/index.html"),
        ]
    };

    for candidate in candidates {
        if let Some(page) = Frontend::get(&candidate) {
            let mime = mime_guess::from_path(&candidate).first_or_octet_stream();

            let mut html =
                String::from_utf8(page.data.to_vec()).expect("embedded HTML must be valid UTF-8");

            if !html.contains(CSRF_PLACEHOLDER) {
                error!(
                    "CSRF placeholder `{}` not found in HTML candidate `{}`",
                    CSRF_PLACEHOLDER, candidate
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Template missing CSRF placeholder",
                )
                    .into_response();
            }

            let token = &user_session.csrf_token;
            html = html.replace(CSRF_PLACEHOLDER, token);

            if html.contains(CSRF_PLACEHOLDER) {
                error!(
                    "CSRF placeholder `{}` still present after replacement in `{}`",
                    CSRF_PLACEHOLDER, candidate
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to inject CSRF token",
                )
                    .into_response();
            }

            return Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", mime.as_ref())
                .body(Body::from(html))
                .unwrap();
        }
    }

    // ---------- 3) Svelte's 404.html ----------
    if let Some(not_found) = Frontend::get("404.html") {
        let mime = mime_guess::from_path("404.html").first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", mime.as_ref())
            .body(Body::from(not_found.data))
            .unwrap()
            .into_response();
    }

    StatusCode::NOT_FOUND.into_response()
}
