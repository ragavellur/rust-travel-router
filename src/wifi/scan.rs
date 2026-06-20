use serde::Serialize;
use std::process::Command;
use std::time::Duration;
use crate::wifi::Backend;

#[derive(Debug, Default, Serialize)]
pub struct Network {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub channel: String,
}

pub fn scan(backend: &Backend, iface: &str) -> Vec<Network> {
    match backend {
        Backend::NetworkManager => scan_nmcli(),
        Backend::WpaSupplicant => scan_wpa_cli(iface),
    }
}

fn scan_nmcli() -> Vec<Network> {
    let out = Command::new("nmcli")
        .args(["-m", "multiline", "-f", "SSID,SECURITY,SIGNAL,CHAN", "device", "wifi", "list"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let mut networks = Vec::new();
    let mut current: Option<Network> = None;

    for line in out.lines() {
        if let Some(val) = line.strip_prefix("SSID:").map(|s| s.trim()) {
            if !val.is_empty() && val != "--" {
                current = Some(Network {
                    ssid: val.to_string(),
                    signal: 0,
                    security: String::new(),
                    channel: String::new(),
                });
            }
        } else if let Some(ref mut net) = current {
            if let Some(val) = line.strip_prefix("SECURITY:").map(|s| s.trim()) {
                net.security = if val == "--" { String::new() } else { val.to_string() };
            } else if let Some(val) = line.strip_prefix("SIGNAL:").map(|s| s.trim()) {
                net.signal = val.parse().unwrap_or(0);
            } else if let Some(val) = line.strip_prefix("CHAN:").map(|s| s.trim()) {
                net.channel = val.to_string();
                if !net.ssid.is_empty() {
                    let n = std::mem::take(net);
                    if !networks.iter().any(|x: &Network| x.ssid == n.ssid) {
                        networks.push(n);
                    }
                }
                current = None;
            }
        }
    }
    networks
}

fn scan_wpa_cli(iface: &str) -> Vec<Network> {
    let _ = Command::new("wpa_cli")
        .args(["-i", iface, "scan"])
        .output();

    std::thread::sleep(Duration::from_secs(3));

    let out = Command::new("wpa_cli")
        .args(["-i", iface, "scan_results"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let mut seen = std::collections::HashMap::new();
    for line in out.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }
        let ssid = parts[4].trim();
        if ssid.is_empty() || ssid == "SSID" {
            continue;
        }
        let signal_dbm: i32 = parts.get(2).and_then(|s| s.trim().parse().ok()).unwrap_or(-100);
        let flags = parts.get(3).unwrap_or(&"");
        let signal_pct = ((signal_dbm + 100).clamp(0, 70) as f64 * 100.0 / 70.0) as u8;

        let entry = Network {
            ssid: ssid.to_string(),
            signal: signal_pct.min(100),
            security: parse_security(flags),
            channel: parts.get(1).unwrap_or(&"--").trim().to_string(),
        };

        seen.entry(ssid.to_string()).or_insert(entry);
    }
    seen.into_values().collect()
}

fn parse_security(flags: &str) -> String {
    let mut sec = Vec::new();
    if flags.contains("WPA3") { sec.push("WPA3"); }
    if flags.contains("WPA2") { sec.push("WPA2"); }
    if flags.contains("WPA") && !flags.contains("WPA2") && !flags.contains("WPA3") {
        sec.push("WPA");
    }
    if sec.is_empty() { sec.push("Open"); }
    sec.join(" ")
}
