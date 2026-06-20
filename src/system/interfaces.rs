use serde::Serialize;
use std::process::Command;

#[derive(Debug, Serialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub mac: String,
    pub ip: Option<String>,
    pub link_detected: bool,
    pub speed: Option<String>,
}

pub fn get_interface_info(iface: &str) -> InterfaceInfo {
    let mut info = InterfaceInfo {
        name: iface.to_string(),
        mac: String::new(),
        ip: None,
        link_detected: false,
        speed: None,
    };

    // MAC address from /sys/class/net/<iface>/address
    info.mac = std::fs::read_to_string(format!("/sys/class/net/{iface}/address"))
        .unwrap_or_default()
        .trim()
        .to_string();

    // Carrier / link detect
    info.link_detected = std::fs::read_to_string(format!("/sys/class/net/{iface}/carrier"))
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map(|v| v == 1)
        .unwrap_or(false);

    // Speed (Mbps)
    info.speed = std::fs::read_to_string(format!("/sys/class/net/{iface}/speed"))
        .ok()
        .map(|s| format!("{} Mbps", s.trim()));

    // IP address
    let out = Command::new("ip")
        .args(["-4", "addr", "show", "dev", iface])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    for line in out.lines() {
        if line.trim().starts_with("inet ") {
            info.ip = line
                .trim()
                .split_whitespace()
                .nth(1)
                .map(|s| s.split('/').next().unwrap_or(s).to_string());
            break;
        }
    }

    info
}

pub fn get_all_interfaces() -> Vec<InterfaceInfo> {
    let interfaces = ["wlan0", "wlan1", "eth0", "ap0"];
    interfaces.iter().map(|name| get_interface_info(name)).collect()
}
