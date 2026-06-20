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

fn connect_nmcli(ssid: &str, password: &str, iface: &str) -> Result<String, String> {
    let mut cmd = Command::new("nmcli");
    cmd.args(["device", "wifi", "connect", ssid]);
    if !password.is_empty() {
        cmd.args(["password", password]);
    }
    cmd.args(["ifname", iface]);

    let out = cmd.output().map_err(|e| format!("nmcli error: {e}"))?;
    if out.status.success() {
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

    fs::write(wpa_conf, &new_conf).map_err(|e| format!("Write wpa_conf: {e}"))?;

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
    Err("Connection timeout (check password)".into())
}
