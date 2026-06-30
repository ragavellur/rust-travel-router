use crate::config::{self, Config};
use crate::system::{clients, interfaces, reboot, uptime};
use crate::web::auth;
use crate::web::AppState;
use crate::wifi::{self, connect, scan, status};

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

fn unauthorized_json() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"})))
}

async fn is_authed(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    let cfg = state.config.read().await;
    if cfg.web_password.is_empty() {
        return true;
    }
    crate::web::get_session_token(headers)
        .map(|t| auth::validate_session(&t))
        .unwrap_or(false)
}

#[derive(Serialize)]
struct StatusResponse {
    wifi_connected: bool,
    connected_ssid: Option<String>,
    ap_active: bool,
    ap_ssid: String,
    ap_ip: String,
    ap_channel: u8,
    uplink_ip: Option<String>,
    client_count: usize,
    uptime_secs: u64,
    hostname: String,
    interfaces: Vec<interfaces::InterfaceInfo>,
}

#[derive(Serialize)]
struct ScanResponse {
    networks: Vec<scan::Network>,
    connected_ssid: Option<String>,
}

#[derive(Deserialize)]
struct ConnectRequest {
    ssid: String,
    password: Option<String>,
    persist: Option<bool>,
}

#[derive(Serialize)]
struct ConnectResponse {
    success: bool,
    message: String,
}

#[derive(Serialize)]
struct ClientsResponse {
    clients: Vec<clients::DhcpClient>,
}

#[derive(Serialize)]
struct LogsResponse {
    logs: String,
}

#[derive(Deserialize)]
struct LogsQuery {
    lines: Option<u32>,
}

#[derive(Deserialize)]
struct LoginRequest {
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    success: bool,
    error: Option<String>,
    session: Option<String>,
}

#[derive(Deserialize)]
struct ConfigUpdate {
    ap_ssid: Option<String>,
    ap_password: Option<String>,
    ap_ip: Option<String>,
    ap_netmask: Option<String>,
    ap_channel: Option<u8>,
    ap_band: Option<String>,
    dhcp_start: Option<String>,
    dhcp_end: Option<String>,
    dhcp_lease_hours: Option<u32>,
    hostname: Option<String>,
    sta_ssid: Option<String>,
    sta_password: Option<String>,
    web_password: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/status", get(api_status))
        .route("/api/scan", get(api_scan))
        .route("/api/connect", post(api_connect))
        .route("/api/config", get(api_config_get).post(api_config_post))
        .route("/api/clients", get(api_clients))
        .route("/api/reboot", post(api_reboot))
        .route("/api/shutdown", post(api_shutdown))
        .route("/api/login", post(api_login))
        .route("/api/logout", post(api_logout))
        .route("/api/logs", get(api_logs))
        .route("/api/reset", post(api_reset))
}

async fn api_status(State(state): State<AppState>, _headers: axum::http::HeaderMap) -> Json<StatusResponse> {
    let cfg = state.config.read().await;
    let backend = wifi::detect_backend(&cfg.wifi_backend);
    let link = status::get_link_status(&backend, &cfg.sta_interface);
    let uplink_ip = if link.connected {
        status::get_uplink_ip(&cfg.sta_interface)
    } else {
        None
    };
    let upt = uptime::get_uptime();
    let ifaces = interfaces::get_all_interfaces();

    Json(StatusResponse {
        wifi_connected: link.connected,
        connected_ssid: link.ssid,
        ap_active: crate::ap::hostapd::is_running() || crate::ap::networkmanager::is_running(),
        ap_ssid: cfg.ap_ssid.clone(),
        ap_ip: cfg.ap_ip.clone(),
        ap_channel: cfg.ap_channel,
        uplink_ip,
        client_count: clients::client_count(),
        uptime_secs: upt.as_secs(),
        hostname: cfg.hostname.clone(),
        interfaces: ifaces,
    })
}

async fn api_scan(State(state): State<AppState>, _headers: axum::http::HeaderMap) -> Json<ScanResponse> {
    let cfg = state.config.read().await;
    let backend = wifi::detect_backend(&cfg.wifi_backend);
    let nets = scan::scan(&backend, &cfg.sta_interface);
    let link = status::get_link_status(&backend, &cfg.sta_interface);
    Json(ScanResponse {
        networks: nets,
        connected_ssid: link.ssid,
    })
}

async fn api_connect(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ConnectRequest>,
) -> Result<Json<ConnectResponse>, (StatusCode, Json<ConnectResponse>)> {
    if !is_authed(&state, &headers).await {
        return Err((StatusCode::UNAUTHORIZED, Json(ConnectResponse { success: false, message: "Unauthorized".into() })));
    }
    if req.ssid.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ConnectResponse { success: false, message: "SSID is required".into() }),
        ));
    }
    let cfg = state.config.read().await;
    let backend = wifi::detect_backend(&cfg.wifi_backend);
    match connect::connect(&backend, &req.ssid, req.password.as_deref().unwrap_or(""), &cfg.sta_interface) {
        Ok(msg) => {
            if req.persist.unwrap_or(false) {
                drop(cfg);
                let mut cfg = state.config.write().await;
                cfg.sta_ssid = req.ssid.clone();
                cfg.sta_password = req.password.unwrap_or_default();
                let path = std::path::Path::new("/etc/travel-net/config.json");
                config::save(path, &cfg).ok();
            }
            Ok(Json(ConnectResponse { success: true, message: msg }))
        }
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ConnectResponse { success: false, message: e }),
        )),
    }
}

async fn api_config_get(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !is_authed(&state, &headers).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response();
    }
    let cfg = state.config.read().await;
    (StatusCode::OK, Json(cfg.clone())).into_response()
}

async fn api_config_post(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(update): Json<ConfigUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    let mut cfg = state.config.write().await;

    if let Some(v) = update.ap_ssid { cfg.ap_ssid = v; }
    if let Some(v) = update.ap_password { cfg.ap_password = v; }
    if let Some(v) = update.ap_ip { cfg.ap_ip = v; }
    if let Some(v) = update.ap_netmask { cfg.ap_netmask = v; }
    if let Some(v) = update.ap_channel { cfg.ap_channel = v; }
    if let Some(v) = update.ap_band { cfg.ap_band = v; }
    if let Some(v) = update.dhcp_start { cfg.dhcp_start = v; }
    if let Some(v) = update.dhcp_end { cfg.dhcp_end = v; }
    if let Some(v) = update.dhcp_lease_hours { cfg.dhcp_lease_hours = v; }
    if let Some(v) = update.hostname { cfg.hostname = v; }
    if let Some(v) = update.sta_ssid { cfg.sta_ssid = v; }
    if let Some(v) = update.sta_password { cfg.sta_password = v; }
    if let Some(v) = update.web_password {
        cfg.web_password = v.clone();
        if v.is_empty() {
            auth::clear_all_sessions();
        } else {
            auth::set_password(&v);
        }
    }

    if let Err(errors) = cfg.validate() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": errors.join("; ")})),
        ));
    }

    let path = std::path::Path::new("/etc/travel-net/config.json");
    if let Err(e) = config::save(path, &cfg) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e.to_string()})),
        ));
    }

    let new_cfg = cfg.clone();
    drop(cfg);
    tokio::spawn(async move {
        crate::ap::apply::apply_config(path, new_cfg).await.ok();
    });

    Ok(Json(serde_json::json!({"success": true, "message": "Configuration saved. AP restarted."})))
}

async fn api_clients(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !is_authed(&state, &headers).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response();
    }
    (StatusCode::OK, Json(ClientsResponse { clients: clients::get_clients() })).into_response()
}

async fn api_reboot(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !is_authed(&state, &headers).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response();
    }
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        reboot::reboot().ok();
    });
    (StatusCode::OK, Json(serde_json::json!({"success": true, "message": "Rebooting..."}))).into_response()
}

async fn api_shutdown(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !is_authed(&state, &headers).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response();
    }
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        reboot::shutdown().ok();
    });
    (StatusCode::OK, Json(serde_json::json!({"success": true, "message": "Shutting down..."}))).into_response()
}

async fn api_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Response {
    let cfg = state.config.read().await;
    let resp = if cfg.web_password.is_empty() || auth::verify_password(&req.password) {
        let session = auth::create_session();
        let cookie = format!("session={session}; Path=/; HttpOnly; SameSite=Lax; Max-Age=1800");
        (
            StatusCode::OK,
            [(header::SET_COOKIE, cookie)],
            Json(LoginResponse { success: true, error: None, session: Some(session) }),
        )
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::SET_COOKIE, String::new())],
            Json(LoginResponse { success: false, error: Some("Invalid password".into()), session: None }),
        )
    };
    resp.into_response()
}

async fn api_logout() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Set-Cookie", "session=; Path=/; Max-Age=0")],
        Json(serde_json::json!({"success": true})),
    )
}

async fn api_logs(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<LogsQuery>,
) -> impl IntoResponse {
    if !is_authed(&state, &headers).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response();
    }
    let lines = query.lines.unwrap_or(100);
    match std::process::Command::new("journalctl")
        .args(["-u", "travel-net", "--no-pager", "-n", &lines.to_string(), "-o", "short-precise"])
        .output()
    {
        Ok(out) => {
            let logs = if out.status.success() {
                String::from_utf8_lossy(&out.stdout).to_string()
            } else {
                String::from_utf8_lossy(&out.stderr).to_string()
            };
            (StatusCode::OK, Json(LogsResponse { logs })).into_response()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(LogsResponse { logs: format!("journalctl error: {e}") })).into_response()
        }
    }
}

async fn api_reset(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }

    let default_cfg = Config::default();
    let path = std::path::Path::new("/etc/travel-net/config.json");

    config::save(path, &default_cfg).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "error": e.to_string()})))
    })?;

    let mut cfg = state.config.write().await;
    *cfg = default_cfg.clone();
    drop(cfg);

    auth::clear_all_sessions();
    auth::set_password("");

    tokio::spawn(async move {
        crate::ap::apply::apply_config(path, default_cfg).await.ok();
    });

    Ok(Json(serde_json::json!({"success": true, "message": "Factory reset done. Services restarting with defaults."})))
}
