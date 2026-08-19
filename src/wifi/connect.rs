use crate::wifi::Backend;
use regex;
use std::fs;
use std::process::Command;
use std::time::Duration;

pub fn connect(backend: &Backend, ssid: &str, password: &str, iface: &str) -> Result<String, String> {
    match backend {
        Backend::NetworkManager => connect_nmcli(ssid, password, iface),
        Backend::WpaSupplicant => connect_wpa_cli(ssid, password, iface),
    }
}

pub fn disconnect(backend: &Backend, iface: &str, ssid: Option<&str>) -> Result<String, String> {
    match backend {
        Backend::NetworkManager => disconnect_nm(iface, ssid),
        Backend::WpaSupplicant => disconnect_wpa(iface, ssid),
    }
}

fn connect_nmcli(ssid: &str, password: &str, iface: &str) -> Result<String, String> {
    let mut cmd = Command::new("nmcli");
    cmd.args(["device", "wifi", "connect", ssid]);
    if !password.is_empty() {
        cmd.args(["password", password]);
    }
    cmd.args(["ifname", iface]);

    let out = cmd.output().map_err(|e| format!("nmcli error: {e}"))?;
    if out.status.success() {
        crate::system::remount::persist_nm_connections();
        Ok(format!("Connected to {ssid}"))
    } else {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        Err(if err.contains("secrets") || err.contains("invalid") {
            "Invalid password".into()
        } else {
            err
        })
    }
}

fn connect_wpa_cli(ssid: &str, password: &str, iface: &str) -> Result<String, String> {
    let wpa_conf = "/etc/wpa_supplicant/wpa_supplicant.conf";

    let pw_out = Command::new("wpa_passphrase")
        .args([ssid, password])
        .output()
        .map_err(|e| format!("wpa_passphrase error: {e}"))?;

    if !pw_out.status.success() {
        return Err(String::from_utf8_lossy(&pw_out.stderr).into());
    }

    let existing = fs::read_to_string(wpa_conf).unwrap_or_default();

    // Remove existing block for this SSID
    let escaped_ssid = regex::escape(ssid);
    let pattern = format!(r#"network=\{{[^}}]*ssid="{}"[^}}]*\}}"#, escaped_ssid);
    let re = regex::Regex::new(&pattern).unwrap();
    let cleaned = re.replace_all(&existing, "");

    let mut new_conf = cleaned.trim().to_string();
    new_conf.push('\n');
    new_conf.push_str(&String::from_utf8_lossy(&pw_out.stdout));

    // Write to tmpfs (bind-mounted or direct)
    crate::system::remount::safe_write(std::path::Path::new(wpa_conf), &new_conf)?;

    let _ = Command::new("wpa_cli")
        .args(["-i", iface, "reconfigure"])
        .output();

    // Wait for connection
    for _ in 0..15 {
        std::thread::sleep(Duration::from_secs(1));
        let link = Command::new("iw")
            .args(["dev", iface, "link"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
        if link.contains("Connected to") {
            return Ok(format!("Connected to {ssid}"));
        }
    }
    // Timeout — stop wpa_supplicant so it doesn't hog the radio forever
    tracing::warn!("STA connection to {ssid} timed out, stopping wpa_supplicant");
    let _ = Command::new("wpa_cli").args(["-i", iface, "terminate"]).output();
    Err("Connection timeout (check password or SSID availability)".into())
}

fn disconnect_nm(iface: &str, ssid: Option<&str>) -> Result<String, String> {
    if let Some(ssid) = ssid {
        let out = Command::new("nmcli")
            .args(["connection", "delete", ssid])
            .output()
            .map_err(|e| format!("nmcli error: {e}"))?;
        if out.status.success() {
            crate::system::remount::persist_nm_connections();
            return Ok(format!("Disconnected and forgot {ssid}"));
        }
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        if err.contains("active") {
            let _ = Command::new("nmcli")
                .args(["device", "disconnect", iface])
                .output();
            std::thread::sleep(Duration::from_secs(1));
            let out2 = Command::new("nmcli")
                .args(["connection", "delete", ssid])
                .output()
                .map_err(|e| format!("nmcli error: {e}"))?;
            if out2.status.success() {
                crate::system::remount::persist_nm_connections();
                return Ok(format!("Disconnected and forgot {ssid}"));
            }
            return Err(String::from_utf8_lossy(&out2.stderr).into());
        }
        Err(err)
    } else {
        let out = Command::new("nmcli")
            .args(["device", "disconnect", iface])
            .output()
            .map_err(|e| format!("nmcli error: {e}"))?;
        if out.status.success() {
            Ok(format!("Disconnected {iface}"))
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into())
        }
    }
}

fn disconnect_wpa(iface: &str, ssid: Option<&str>) -> Result<String, String> {
    if let Some(ssid) = ssid {
        let wpa_conf = "/etc/wpa_supplicant/wpa_supplicant.conf";
        let existing = fs::read_to_string(wpa_conf).unwrap_or_default();
        let escaped_ssid = regex::escape(ssid);
        let pattern = format!(r#"network=\{{[^}}]*ssid="{}"[^}}]*\}}"#, escaped_ssid);
        let re = regex::Regex::new(&pattern).unwrap();
        let cleaned = re.replace_all(&existing, "");
        let new_conf = cleaned.trim().to_string() + "\n";

        crate::system::remount::safe_write(std::path::Path::new(wpa_conf), &new_conf)?;

        let _ = Command::new("wpa_cli")
            .args(["-i", iface, "reconfigure"])
            .output();
        Ok(format!("Disconnected and forgot {ssid}"))
    } else {
        let _ = Command::new("wpa_cli")
            .args(["-i", iface, "disconnect"])
            .output();
        Ok(format!("Disconnected {iface}"))
    }
}
