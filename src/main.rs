mod config;
mod system;
mod wifi;
mod ap;
mod dhcp;
mod firewall;
mod vpn;
mod web;
mod templates;

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "travel-net", version, about = "Travel NAT Router")]
struct Cli {
    #[arg(short, long, default_value = "/etc/travel-net/config.json")]
    config: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let cfg = config::load(&cli.config).unwrap_or_else(|e| {
        tracing::error!("Failed to load config: {e}");
        std::process::exit(1);
    });

    let backend = wifi::detect_backend(&cfg.wifi_backend);
    tracing::info!("WiFi backend: {backend:?}");

    let uplink = firewall::detect_uplink(&cfg);
    tracing::info!("Uplink interface: {uplink}");
    firewall::apply_performance_tuning(&cfg);

    // Connect STA so the AP can match its channel. On single-radio radios
    // (brcmfmac NEO Air), the AP MUST run on the same channel as the STA.
    // The wait is bounded (~8s) so the AP still comes up quickly even if no
    // network is available; a late connection is picked up by the AP monitor.
    if !cfg.sta_ssid.is_empty() {
        let backend = wifi::detect_backend(&cfg.wifi_backend);
        let sta_iface = cfg.sta_interface.clone();
        let sta_ssid = cfg.sta_ssid.clone();
        let sta_password = cfg.sta_password.clone();
        let cfg_for_tuning = cfg.clone();
        tracing::info!("Connecting to uplink STA: {sta_ssid}");
        let task = tokio::task::spawn_blocking(move || {
            let result = wifi::connect::connect(&backend, &sta_ssid, &sta_password, &sta_iface);
            firewall::apply_performance_tuning(&cfg_for_tuning);
            result
        });
        let _ = tokio::time::timeout(std::time::Duration::from_secs(8), task).await;
    }

    // Start AP — it auto-detects the STA channel when connected.
    if let Err(e) = ap::start_ap(&cfg).await {
        tracing::error!("Failed to start AP: {e}");
    }
    let nm_backend = backend == wifi::Backend::NetworkManager;
    if !nm_backend {
        let _ = dhcp::start_dnsmasq(&cfg).await;
        let _ = firewall::apply_ruleset(&cfg).await;
    }

    // Monitor the STA channel and keep the AP in sync (single-radio devices).
    // If the AP ever dies or the user connects to a new WiFi network, the
    // monitor restarts the AP on the correct channel.
    ap::start_ap_channel_monitor(cfg.clone());

    // Start the saved VPN (Tailscale / WireGuard). Must come after the
    // firewall ruleset so the dedicated travel-vpn nft table survives the
    // ruleset flush. Re-running at boot also re-auths Tailscale when its
    // state was wiped (e.g. read-only rootfs with /var/lib/tailscale on tmpfs).
    if let Err(e) = vpn::apply(&cfg) {
        tracing::error!("VPN apply failed (backend={}): {e}", cfg.vpn.backend);
    } else {
        tracing::info!("VPN applied (backend={})", cfg.vpn.backend);
    }

    let app = web::build_router(cfg.clone());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:80")
        .await
        .expect("Bind port 80");

    tracing::info!("Web UI listening on http://0.0.0.0:80");
    axum::serve(listener, app).await.unwrap();
}
