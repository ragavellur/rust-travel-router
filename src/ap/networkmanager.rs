use crate::config::Config;
use std::process::Command;

const NM_CONNECTION_NAME: &str = "travel-net-hotspot";

pub async fn start_nm_ap(cfg: &Config) -> Result<(), String> {
    let iface = &cfg.ap_interface;

    // Compute CIDR from bare IP + netmask, or use directly if already CIDR
    let cidr = if cfg.ap_ip.contains('/') {
        cfg.ap_ip.clone()
    } else {
        let prefix = ipnetwork::ipv4_mask_to_prefix(
            cfg.ap_netmask.parse()
                .map_err(|e| format!("Invalid netmask: {e}"))?
        ).map_err(|e| format!("Invalid netmask: {e}"))?;
        format!("{}/{}", cfg.ap_ip, prefix)
    };

    // Create virtual AP interface if it doesn't exist
    let check = Command::new("iw")
        .args(["dev"])
        .output()
        .map_err(|e| format!("iw error: {e}"))?;
    let output = String::from_utf8_lossy(&check.stdout);
    if !output.contains(&format!("Interface {iface}")) {
        let result = Command::new("iw")
            .args(["phy", "phy0", "interface", "add", iface, "type", "managed"])
            .output()
            .map_err(|e| format!("iw add failed: {e}"))?;
        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("Failed to create {iface}: {stderr}"));
        }
        tracing::info!("Created AP interface {iface}");
    }

    // Assign IP and bring up
    let _ = Command::new("ip").args(["addr", "flush", "dev", iface]).output();
    Command::new("ip")
        .args(["addr", "add", &cidr, "dev", iface])
        .output()
        .map_err(|e| format!("ip addr add failed: {e}"))?;
    Command::new("ip")
        .args(["link", "set", iface, "up"])
        .output()
        .map_err(|e| format!("ip link set up failed: {e}"))?;

    // Remove existing travel-net hotspot connection profile
    let _ = Command::new("nmcli")
        .args(["connection", "delete", NM_CONNECTION_NAME])
        .output();

    // Map ap_band to NM wifi.band
    let nm_band = match cfg.ap_band.as_str() {
        "a" => Some("a"),
        "auto" => None,
        _ => Some("bg"),
    };
    let channel_str = cfg.ap_channel.to_string();
    let mut args = vec![
        "connection", "add",
        "type", "wifi",
        "mode", "ap",
        "con-name", NM_CONNECTION_NAME,
        "ifname", iface,
        "ssid", &cfg.ap_ssid,
        "wifi.channel", &channel_str,
        "ipv4.method", "shared",
        "ipv4.address", &cidr,
    ];
    if let Some(band) = nm_band {
        args.push("wifi.band");
        args.push(band);
    }
    if !cfg.ap_password.is_empty() {
        args.push("wifi-sec.key-mgmt");
        args.push("wpa-psk");
        args.push("wifi-sec.psk");
        args.push(&cfg.ap_password);
    }

    let result = Command::new("nmcli")
        .args(&args)
        .output()
        .map_err(|e| format!("nmcli add failed: {e}"))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("Failed to create NM hotspot: {stderr}"));
    }

    // Activate hotspot
    let result = Command::new("nmcli")
        .args(["connection", "up", NM_CONNECTION_NAME])
        .output()
        .map_err(|e| format!("nmcli up failed: {e}"))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("Failed to activate NM hotspot: {stderr}"));
    }

    tracing::info!("NM hotspot started (SSID: {}, iface: {})", cfg.ap_ssid, iface);
    Ok(())
}

pub async fn stop_nm_ap() {
    let _ = Command::new("nmcli")
        .args(["connection", "down", NM_CONNECTION_NAME])
        .output();
    let _ = Command::new("nmcli")
        .args(["connection", "delete", NM_CONNECTION_NAME])
        .output();
}

pub fn is_running() -> bool {
    Command::new("nmcli")
        .args(["-t", "-f", "NAME", "connection", "show", "--active"])
        .output()
        .map(|o| {
            let out = String::from_utf8_lossy(&o.stdout);
            out.lines().any(|l| l.trim() == NM_CONNECTION_NAME)
        })
        .unwrap_or(false)
}
