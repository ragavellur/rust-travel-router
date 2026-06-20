use crate::config::Config;
use std::process::Command;

pub async fn create_ap_interface(cfg: &Config) -> Result<(), String> {
    let iface = &cfg.ap_interface;

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
