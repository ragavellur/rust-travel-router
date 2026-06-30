pub mod hostapd;
pub mod interface;
pub mod apply;
pub mod networkmanager;

use crate::config::Config;
use crate::wifi;

pub fn assign_ap_ip(cfg: &Config) -> Result<(), String> {
    let iface = &cfg.ap_interface;
    let ip = &cfg.ap_ip;

    let prefix = ipnetwork::Ipv4Network::new(
        ip.parse().map_err(|e| format!("Invalid ap_ip {ip}: {e}"))?,
        ipnetwork::ipv4_mask_to_prefix(
            cfg.ap_netmask
                .parse()
                .map_err(|e| format!("Invalid ap_netmask {}: {e}", cfg.ap_netmask))?
        ).map_err(|e| format!("Invalid netmask: {e}"))?
    ).map_err(|e| format!("Failed to compute network: {e}"))?;

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
    let backend = wifi::detect_backend();
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
