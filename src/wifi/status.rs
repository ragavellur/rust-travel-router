use crate::wifi::Backend;
use std::process::Command;

pub struct LinkStatus {
    pub connected: bool,
    pub ssid: Option<String>,
}

pub fn get_link_status(backend: &Backend, iface: &str) -> LinkStatus {
    match backend {
        Backend::NetworkManager => status_nmcli(iface),
        Backend::WpaSupplicant => status_iw(iface),
    }
}

pub fn get_uplink_ip(iface: &str) -> Option<String> {
    let out = Command::new("ip")
        .args(["-4", "addr", "show", "dev", iface])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())?;

    for line in out.lines() {
        if line.trim().starts_with("inet ") {
            let ip = line.trim().split_whitespace().nth(1)?.split('/').next()?.to_string();
            return Some(ip);
        }
    }
    None
}

fn status_nmcli(iface: &str) -> LinkStatus {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,STATE", "device", "status"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let connected = out.lines().any(|line| {
        line.starts_with(&format!("{iface}:")) && line.contains("connected")
    });

    // Get SSID via iw (works on NM-managed interfaces too)
    let ssid = if connected {
        let iw_out = Command::new("iw")
            .args(["dev", iface, "link"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
        iw_out.lines()
            .find(|l| l.trim().starts_with("SSID:"))
            .map(|l| l.trim().strip_prefix("SSID:").unwrap_or("").trim().to_string())
    } else {
        None
    };

    LinkStatus { connected, ssid }
}

fn status_iw(iface: &str) -> LinkStatus {
    let out = Command::new("iw")
        .args(["dev", iface, "link"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let connected = out.contains("Connected to");
    let ssid = out.lines()
        .find(|l| l.trim().starts_with("SSID:"))
        .map(|l| l.trim().strip_prefix("SSID:").unwrap_or("").trim().to_string());

    LinkStatus { connected, ssid }
}
