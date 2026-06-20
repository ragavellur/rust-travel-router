use crate::config::{self, Config};
use crate::ap::hostapd;
use crate::dhcp;
use crate::firewall;
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

    // Regenerate and restart services
    hostapd::stop_hostapd().await;

    // Reassign IP in case it changed
    crate::ap::assign_ap_ip(&new_cfg).map_err(|e| vec![e])?;

    hostapd::start_hostapd(&new_cfg).await.map_err(|e| vec![e])?;

    dhcp::stop_dnsmasq().await;
    dhcp::start_dnsmasq(&new_cfg).await.map_err(|e| vec![e])?;

    firewall::apply_ruleset(&new_cfg).await.map_err(|e| vec![e])?;

    tracing::info!("Configuration applied successfully");
    Ok(())
}
