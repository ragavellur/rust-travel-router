use crate::config::{self, Config};
use crate::ap::hostapd;
use crate::dhcp;
use crate::firewall;
use crate::wifi;
use std::path::Path;

pub async fn apply_config(path: &Path, new_cfg: Config) -> Result<(), Vec<String>> {
    new_cfg.validate()?;

    // Backup current config
    if path.exists() {
        let backup = path.with_extension("json.bak");
        std::fs::copy(path, &backup).ok();
    }

    // Save new config
    config::save(path, &new_cfg).map_err(|e| vec![e.to_string()])?;

    // Apply power mode tuning
    firewall::apply_performance_tuning(&new_cfg);

    let backend = wifi::detect_backend(&new_cfg.wifi_backend);
    match backend {
        wifi::Backend::NetworkManager => {
            // stop_nm_ap deletes the NM profile (writes to disk).
            // On ro rootfs, skip delete and just call start_nm_ap which
            // tries 'up' first before falling back to create.
            crate::ap::networkmanager::start_nm_ap(&new_cfg).await.map_err(|e| vec![e])?;
        }
        wifi::Backend::WpaSupplicant => {
            hostapd::stop_hostapd().await;
            crate::ap::assign_ap_ip(&new_cfg).map_err(|e| vec![e])?;
            hostapd::start_hostapd(&new_cfg).await.map_err(|e| vec![e])?;
            dhcp::stop_dnsmasq().await;
            dhcp::start_dnsmasq(&new_cfg).await.map_err(|e| vec![e])?;
            firewall::apply_ruleset(&new_cfg).await.map_err(|e| vec![e])?;
        }
    }

    tracing::info!("Configuration applied successfully");
    Ok(())
}
