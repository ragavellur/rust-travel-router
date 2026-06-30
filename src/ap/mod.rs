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
