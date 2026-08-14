use crate::config::{self, Config, VpnConfig, WgPeer};
use crate::firewall;
use crate::system::{clients, interfaces, reboot, uptime};
use crate::vpn;
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
    uplink_interface: String,
    client_count: usize,
    uptime_secs: u64,
    hostname: String,
    interfaces: Vec<interfaces::InterfaceInfo>,
    vpn: vpn::VpnStatus,
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
struct IpRequest {
    ip: String,
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
    power_mode: Option<String>,
}

#[derive(Deserialize)]
struct VpnUpdate {
    vpn: VpnConfig,
}

#[derive(Deserialize)]
struct ImportRequest {
    conf: String,
}

#[derive(Deserialize)]
struct GenKeysRequest {
    scope: String,
}

#[derive(Deserialize)]
struct PeerRequest {
    action: String,
    name: Option<String>,
    public_key: Option<String>,
    tunnel_ip: Option<String>,
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
        .route("/api/clients/disconnect", post(api_disconnect))
        .route("/api/clients/unblock", post(api_unblock))
        .route("/api/reset", post(api_reset))
        .route("/api/vpn", get(api_vpn_get).post(api_vpn_post))
        .route("/api/vpn/import", post(api_vpn_import))
        .route("/api/vpn/genkeys", post(api_vpn_genkeys))
        .route("/api/vpn/peers", post(api_vpn_peers))
        .route("/api/vpn/install", post(api_vpn_install))
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

    let uplink_iface = firewall::detect_uplink(&cfg);

    Json(StatusResponse {
        wifi_connected: link.connected,
        connected_ssid: link.ssid,
        ap_active: crate::ap::hostapd::is_running() || crate::ap::networkmanager::is_running(),
        ap_ssid: cfg.ap_ssid.clone(),
        ap_ip: cfg.ap_ip.clone(),
        ap_channel: cfg.ap_channel,
        uplink_ip,
        uplink_interface: uplink_iface,
        client_count: clients::client_count(),
        uptime_secs: upt.as_secs(),
        hostname: cfg.hostname.clone(),
        interfaces: ifaces,
        vpn: vpn::status(&cfg),
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
    if let Some(v) = update.ap_netmask { if !v.is_empty() { cfg.ap_netmask = v; } }
    if let Some(v) = update.ap_channel { cfg.ap_channel = v; }
    if let Some(v) = update.ap_band { cfg.ap_band = v; }
    if let Some(v) = update.dhcp_start { cfg.dhcp_start = v; }
    if let Some(v) = update.dhcp_end { cfg.dhcp_end = v; }
    if let Some(v) = update.dhcp_lease_hours { cfg.dhcp_lease_hours = v; }
    if let Some(v) = update.hostname { cfg.hostname = v; }
    if let Some(v) = update.sta_ssid { cfg.sta_ssid = v; }
    if let Some(v) = update.sta_password { cfg.sta_password = v; }
    if let Some(v) = update.power_mode { cfg.power_mode = v; }
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

async fn api_disconnect(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<IpRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    match clients::block_client(&req.ip) {
        Ok(_) => Ok(Json(serde_json::json!({"success": true}))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "error": e})))),
    }
}

async fn api_unblock(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<IpRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    match clients::unblock_client(&req.ip) {
        Ok(_) => Ok(Json(serde_json::json!({"success": true}))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success": false, "error": e})))),
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

fn apply_vpn_in_background(cfg: Config) {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = vpn::apply(&cfg) {
            tracing::error!("VPN apply failed: {e}");
        }
    });
}

async fn api_vpn_get(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    if !is_authed(&state, &headers).await {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Unauthorized"}))).into_response();
    }
    let cfg = state.config.read().await;
    (StatusCode::OK, Json(serde_json::json!({"config": cfg.vpn.clone(), "status": vpn::status(&cfg)}))).into_response()
}

async fn api_vpn_post(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(update): Json<VpnUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    let mut cfg = state.config.write().await;
    cfg.vpn = update.vpn;

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
    apply_vpn_in_background(new_cfg);
    Ok(Json(serde_json::json!({"success": true, "message": "VPN settings saved and applied."})))
}

async fn api_vpn_import(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ImportRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    match vpn::import_conf(&req.conf) {
        Ok(parsed) => Ok(Json(serde_json::json!({
            "success": true,
            "parsed": parsed,
            "message": "Config parsed — review it below, then Save to apply."
        }))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": e})),
        )),
    }
}

async fn api_vpn_genkeys(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<GenKeysRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    let (privkey, pubkey) = vpn::gen_keypair().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e})),
        )
    })?;
    let mut cfg = state.config.write().await;
    match req.scope.as_str() {
        "server" => {
            cfg.vpn.wg_server_private_key = privkey.clone();
            cfg.vpn.wg_server_public_key = pubkey.clone();
        }
        "client" => {
            cfg.vpn.wg_private_key = privkey.clone();
            cfg.vpn.wg_peer_public_key = pubkey.clone();
        }
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success": false, "error": format!("scope must be 'client' or 'server', got '{other}'")})),
            ));
        }
    }
    let path = std::path::Path::new("/etc/travel-net/config.json");
    if let Err(e) = config::save(path, &cfg) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e.to_string()})),
        ));
    }
    Ok(Json(serde_json::json!({
        "success": true,
        "scope": req.scope,
        "private_key": privkey,
        "public_key": pubkey,
    })))
}

async fn api_vpn_peers(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<PeerRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    let name = req
        .name
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": "A peer name is required"})),
        ));
    }
    match req.action.as_str() {
        "add" => {
            let mut cfg = state.config.write().await;
            if cfg.vpn.wg_peers.iter().any(|p| p.name == name) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"success": false, "error": "A peer with that name already exists"})),
                ));
            }
            let (peer, client_conf) = match (req.public_key.as_deref(), req.tunnel_ip.as_deref()) {
                (Some(pk), Some(tip)) => {
                    let peer = WgPeer {
                        name: name.clone(),
                        public_key: pk.trim().to_string(),
                        tunnel_ip: tip.trim().to_string(),
                    };
                    (peer, None)
                }
                _ => {
                    let (peer, conf) = vpn::gen_peer(&cfg, &name).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({"success": false, "error": e})),
                        )
                    })?;
                    (peer, Some(conf))
                }
            };
            cfg.vpn.wg_peers.push(peer.clone());
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
            apply_vpn_in_background(new_cfg);
            Ok(Json(serde_json::json!({
                "success": true,
                "message": "Travel device added.",
                "peer": peer,
                "client_conf": client_conf,
            })))
        }
        "remove" => {
            let mut cfg = state.config.write().await;
            let before = cfg.vpn.wg_peers.len();
            cfg.vpn.wg_peers.retain(|p| p.name != name);
            if cfg.vpn.wg_peers.len() == before {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"success": false, "error": "No peer with that name"})),
                ));
            }
            let path = std::path::Path::new("/etc/travel-net/config.json");
            if let Err(e) = config::save(path, &cfg) {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"success": false, "error": e.to_string()})),
                ));
            }
            let peers = cfg.vpn.wg_peers.clone();
            let new_cfg = cfg.clone();
            drop(cfg);
            apply_vpn_in_background(new_cfg);
            Ok(Json(serde_json::json!({"success": true, "message": "Peer removed.", "peers": peers})))
        }
        other => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": format!("action must be 'add' or 'remove', got '{other}'")})),
        )),
    }
}

async fn api_vpn_install(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !is_authed(&state, &headers).await {
        return Err(unauthorized_json());
    }
    let logs = tokio::task::spawn_blocking(vpn::install)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"success": false, "error": e.to_string()})),
            )
        })?;
    match logs {
        Ok(lines) => Ok(Json(serde_json::json!({"success": true, "logs": lines}))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e})),
        )),
    }
}
