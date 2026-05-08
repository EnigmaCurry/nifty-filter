use std::sync::Arc;

use aide::NoApi;
use aide::axum::IntoApiResponse;
use aide::openapi::{ApiKeyLocation, OpenApi, SecurityScheme};
use aide::transform::TransformOpenApi;
use axum::{Extension, Json};

use aide::axum::{ApiRouter, routing::get};
use aide::redoc::Redoc;
use aide::scalar::Scalar;
use aide::swagger::Swagger;

/// Helper to configure the global OpenAPI document.
/// This is what you pass to `finish_api_with`.
pub fn configure_openapi(api_spec: TransformOpenApi<'_>) -> TransformOpenApi<'_> {
    api_spec
        .title("API Docs")
        .description(r#"
## Client authentication

 1. GET [`/api/whoami`](#tag/session/GET/api/whoami) - retrieve your
    session CSRF token - the response sets the cookie.

 2. You must provide the CSRF token in the `X-CSRF-TOKEN` header in
     all requests using state changing methods
     (POST/PUT/DELETE/PATCH).

    When using the Scalar test page (`/docs`), you must enable
    Authentication in this top section (right below Ferris the crab):

     * Select the `csrfToken` Auth Type from the dropdown menu.
     * Enter your current CSRF token value under `X-CSRF-TOKEN`.

 3. POST [`/api/login`](#tag/auth/POST/api/login) - this upgrades your
    session to become fully authenticated.

 4. Now you may test any other API endpoint you want. Remember that
    the browser must send both the cookie and CSRF token headers in
    all future requests.

## Alternative interfaces

 * [scalar](/docs)
 * [redoc](/docs/redoc)
 * [swagger](/docs/swagger)
"#)
        .security_scheme(
        "csrfToken",
        SecurityScheme::ApiKey {
            location: ApiKeyLocation::Header,
            name: "X-CSRF-Token".to_string(),
            description: Some(
                "CSRF token is required only for state-changing requests (POST/PUT/PATCH/DELETE)"
                    .to_string(),
            ),
            extensions: Default::default(),
        },
    )
    // CSRF is optional for GET requests, so don't require it (but this is how you could):
    //.security_requirement("csrfToken")
}

pub fn docs_routes() -> ApiRouter {
    aide::generate::infer_responses(true);

    let router = ApiRouter::new()
        // UI routes: NOT in OpenAPI, use plain `route` + `get`
        .route(
            "/",
            get(Scalar::new("/docs/api.json")
                .with_title("Aide Axum")
                .axum_handler()),
        )
        .route(
            "/redoc",
            get(Redoc::new("/docs/api.json")
                .with_title("Aide Axum")
                .axum_handler()),
        )
        .route(
            "/swagger",
            get(Swagger::new("/docs/api.json")
                .with_title("Aide Axum")
                .axum_handler()),
        )
        // Spec route: this one *is* part of the OpenAPI schema
        .api_route("/api.json", get(serve_docs));

    aide::generate::infer_responses(false);

    router
}

async fn serve_docs(Extension(api): Extension<Arc<OpenApi>>) -> impl IntoApiResponse {
    NoApi(Json(api))
}
