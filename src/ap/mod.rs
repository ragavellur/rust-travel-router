pub mod channel;
pub mod hostapd;
pub mod interface;
pub mod apply;
pub mod networkmanager;

use crate::config::Config;
use crate::wifi;

pub fn assign_ap_ip(cfg: &Config) -> Result<(), String> {
    let iface = &cfg.ap_interface;

    let prefix = if cfg.ap_ip.contains('/') {
        cfg.ap_ip.parse::<ipnetwork::Ipv4Network>()
            .map_err(|e| format!("Invalid ap_ip {}: {e}", cfg.ap_ip))?
    } else {
        let ip: std::net::Ipv4Addr = cfg.ap_ip.parse()
            .map_err(|e| format!("Invalid ap_ip {}: {e}", cfg.ap_ip))?;
        let mask: std::net::Ipv4Addr = cfg.ap_netmask.parse()
            .map_err(|e| format!("Invalid ap_netmask {}: {e}", cfg.ap_netmask))?;
        let prefix_len = ipnetwork::ipv4_mask_to_prefix(mask)
            .map_err(|e| format!("Invalid netmask: {e}"))?;
        ipnetwork::Ipv4Network::new(ip, prefix_len)
            .map_err(|e| format!("Failed to compute network: {e}"))?
    };
    let cidr = format!("{}", prefix);

    // Remove any existing IP on the interface (idempotent)
    let _ = std::process::Command::new("ip")
        .args(["addr", "flush", "dev", iface])
        .output();

    // Assign IP and bring interface up
    std::process::Command::new("ip")
        .args(["addr", "add", &cidr, "dev", iface])
        .output()
        .map_err(|e| format!("ip addr add failed: {e}"))?;

    std::process::Command::new("ip")
        .args(["link", "set", iface, "up"])
        .output()
        .map_err(|e| format!("ip link set up failed: {e}"))?;

    Ok(())
}

pub async fn start_ap(cfg: &Config) -> Result<(), String> {
    let backend = wifi::detect_backend(&cfg.wifi_backend);
    match backend {
        wifi::Backend::NetworkManager => {
            networkmanager::start_nm_ap(cfg).await
        }
        wifi::Backend::WpaSupplicant => {
            interface::create_ap_interface(cfg).await?;
            assign_ap_ip(cfg)?;
            hostapd::start_hostapd(cfg).await?;
            Ok(())
        }
    }
}

/// Spawn a background task that watches the STA interface's channel.
/// If the STA channel changes (e.g. user connects to a new WiFi network),
/// the AP is restarted on the new channel so both stay in sync.
/// This only matters for single-radio (brcmfmac/AIC) devices where AP+STA
/// must share a channel. For NM-backed devices NM handles co-existence,
/// so this is a no-op there.
pub fn start_ap_channel_monitor(cfg: Config) {
    let backend = wifi::detect_backend(&cfg.wifi_backend);
    if backend == wifi::Backend::NetworkManager {
        // NM manages AP/STA channel co-existence on its own radio.
        return;
    }

    let sta_iface = cfg.sta_interface.clone();
    let ap_iface = cfg.ap_interface.clone();
    let task_cfg = cfg.clone();

    tokio::task::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            // Verify the AP is actually running. If it died (e.g. boot race or
            // a failed channel attempt), restart it regardless of STA state.
            if !hostapd::is_running() {
                tracing::warn!("AP monitor: hostapd not running, restarting AP");
                let (ch, _) = channel::resolve_ap_channel(
                    task_cfg.ap_channel, &task_cfg.ap_band, &sta_iface, &sta_iface,
                );
                stop_ap(&ap_iface).await;
                match restart_ap_on_channel(&task_cfg, ch).await {
                    Ok(()) => tracing::info!("AP monitor: AP restarted on channel {ch}"),
                    Err(e) => tracing::warn!("AP monitor: restart failed ({ch}): {e}"),
                }
                continue;
            }

            let sta = channel::detect_sta_channel(&sta_iface);
            match sta {
                Some((ch, _band)) => {
                    let actual = channel::detect_ap_channel(&ap_iface);
                    if actual == Some(ch) {
                        // AP is on the STA channel; nothing to do.
                    } else if let Some(actual_ch) = actual {
                        // Single-radio: hostapd can "start" on a configured channel
                        // that differs from the STA channel, but the firmware keeps
                        // the radio on the STA channel. Beacons then advertise the
                        // wrong channel to clients. Resync the AP onto the STA channel.
                        tracing::info!(
                            "AP channel mismatch (AP on {actual_ch}, STA on {ch}), restarting AP on {ch}"
                        );
                        stop_ap(&ap_iface).await;
                        if let Err(e) = restart_ap_on_channel(&task_cfg, ch).await {
                            tracing::warn!("Failed to sync AP to channel {ch}: {e}");
                        }
                    } else {
                        // AP interface not yet reporting a channel; wait for next pass.
                    }
                }
                None => {
                    // No STA connected; standalone AP is fine on its current channel.
                }
            }
        }
    });
}

async fn stop_ap(ap_iface: &str) {
    let _ = std::process::Command::new("ip").args(["link", "set", ap_iface, "down"]).output();
    let _ = hostapd::stop_hostapd().await;
    let _ = crate::dhcp::stop_dnsmasq().await;
}

async fn restart_ap_on_channel(cfg: &Config, channel: u8) -> Result<(), String> {
    // Delete any stale interface first: after a failed hostapd start the
    // interface is left in DISABLED state and reusing it fails forever.
    let _ = std::process::Command::new("iw")
        .args(["dev", &cfg.ap_interface, "del"])
        .output();
    crate::ap::interface::create_ap_interface(cfg).await?;
    assign_ap_ip(cfg)?;
    crate::ap::hostapd::generate_conf_with(cfg, channel)?;
    crate::ap::hostapd::spawn_hostapd(cfg).await?;
    let _ = crate::dhcp::start_dnsmasq(cfg).await;
    Ok(())
}
