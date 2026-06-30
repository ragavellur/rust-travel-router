use crate::config::Config;
use std::path::Path;
use std::process::Command;

const NM_UNMANAGED_CONF: &str = "/etc/NetworkManager/conf.d/98-travel-net-unmanaged.conf";

fn nm_mark_unmanaged(iface: &str) {
    if !Path::new("/usr/bin/nmcli").exists() {
        return;
    }
    // Create NM config to ignore this interface permanently
    let conf = format!("[keyfile]\nunmanaged-devices=interface-name:{iface}\n");
    let _ = std::fs::write(NM_UNMANAGED_CONF, &conf);
    let _ = Command::new("nmcli").args(["general", "reload"]).output();
}

pub async fn create_ap_interface(cfg: &Config) -> Result<(), String> {
    let iface = &cfg.ap_interface;

    // Mark interface as unmanaged in NM before creating it
    nm_mark_unmanaged(iface);

    // Check if interface already exists
    let check = Command::new("iw")
        .args(["dev"])
        .output()
        .map_err(|e| format!("iw error: {e}"))?;
    let output = String::from_utf8_lossy(&check.stdout);
    if output.contains(&format!("Interface {iface}")) {
        tracing::info!("AP interface {iface} already exists");
        return Ok(());
    }

    // Try to create virtual interface
    let result = Command::new("iw")
        .args(["phy", "phy0", "interface", "add", iface, "type", "__ap"])
        .output();

    match result {
        Ok(out) if out.status.success() => {
            tracing::info!("Created AP interface {iface}");
            Ok(())
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // Some drivers (AIC8800) don't support type __ap; try type managed
            if stderr.contains("Invalid argument") || stderr.contains("Not supported") {
                tracing::warn!("type __ap failed (unsupported driver), trying type managed");
                Command::new("iw")
                    .args(["phy", "phy0", "interface", "add", iface, "type", "managed"])
                    .output()
                    .map_err(|e| format!("iw managed add error: {e}"))?;
                tracing::info!("Created AP interface {iface} with type managed");
                Ok(())
            } else {
                Err(format!("Failed to create AP interface: {stderr}"))
            }
        }
        Err(e) => Err(format!("iw command failed: {e}")),
    }
}

pub async fn delete_ap_interface(cfg: &Config) -> Result<(), String> {
    Command::new("iw")
        .args(["dev", &cfg.ap_interface, "del"])
        .output()
        .map_err(|e| format!("Failed to delete AP interface: {e}"))?;
    Ok(())
}
