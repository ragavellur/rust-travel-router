pub mod auth;
pub mod pages;
pub mod api;

use crate::config::Config;
use axum::{
    http::header, response::{IntoResponse, Redirect, Response}, Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<tokio::sync::RwLock<Config>>,
}

pub fn get_session_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie.split(';') {
        let mut parts = pair.trim().splitn(2, '=');
        if parts.next()? == "session" {
            return Some(parts.next()?.to_string());
        }
    }
    None
}

pub async fn check_auth(state: &AppState, headers: &axum::http::HeaderMap) -> Result<(), Response> {
    let cfg = state.config.read().await;
    if cfg.web_password.is_empty() {
        return Ok(());
    }
    let session_ok = get_session_token(headers)
        .map(|t| auth::validate_session(&t))
        .unwrap_or(false);
    if session_ok {
        Ok(())
    } else {
        Err(Redirect::to("/login").into_response())
    }
}

pub fn build_router(cfg: Config) -> Router {
    if !cfg.web_password.is_empty() {
        auth::set_password(&cfg.web_password);
    }

    let state = AppState {
        config: Arc::new(tokio::sync::RwLock::new(cfg)),
    };

    Router::new()
        .merge(pages::routes())
        .merge(api::routes())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
