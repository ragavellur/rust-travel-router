use crate::templates;
use crate::web::{check_auth, AppState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/scan", get(scan))
        .route("/config", get(config))
        .route("/setup", get(setup))
        .route("/login", get(login_page))
        .route("/logs", get(logs_page))
        .route("/favicon.png", get(favicon))
        .route("/static/style.css", get(style_css))
        .route("/hotspot-detect.html", get(index))
        .route("/generate_204", get(generate_204))
        .fallback(handler_404)
}

async fn index(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = check_auth(&state, &headers).await { return r; }
    Html(templates::INDEX_HTML.to_string()).into_response()
}

async fn scan(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = check_auth(&state, &headers).await { return r; }
    Html(templates::SCAN_HTML.to_string()).into_response()
}

async fn config(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = check_auth(&state, &headers).await { return r; }
    Html(templates::CONFIG_HTML.to_string()).into_response()
}

async fn setup(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = check_auth(&state, &headers).await { return r; }
    Html(templates::SETUP_HTML.to_string()).into_response()
}

async fn logs_page(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = check_auth(&state, &headers).await { return r; }
    Html(templates::LOGS_HTML.to_string()).into_response()
}

async fn login_page() -> Html<&'static str> {
    Html(templates::LOGIN_HTML)
}

async fn favicon() -> impl IntoResponse {
    ([("Content-Type", "image/svg+xml")], templates::FAVICON_SVG.as_bytes())
}

async fn style_css() -> impl IntoResponse {
    ([("Content-Type", "text/css")], templates::STYLE_CSS)
}

async fn generate_204() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn handler_404() -> impl IntoResponse {
    let body = templates::ERROR_HTML
        .replace("{code}", "404")
        .replace("{message}", "Page not found");
    (StatusCode::NOT_FOUND, [("Content-Type", "text/html")], body)
}
