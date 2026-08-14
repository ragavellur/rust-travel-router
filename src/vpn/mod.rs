use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;

pub const VPN_NFT_FILE: &str = "/etc/travel-net/travel-vpn.nft";

#[derive(Debug, Clone, Default, Serialize)]
pub struct InstalledStatus {
    pub tailscale: bool,
    pub tailscaled_active: bool,
    pub wg: bool,
    pub wg_quick: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TailscaleStatus {
    pub state: String,
    pub hostname: String,
    pub ip: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WireguardStatus {
    pub iface_up: bool,
    pub endpoint: String,
    pub handshake_age_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VpnStatus {
    pub backend: String,
    pub role: String,
    pub installed: InstalledStatus,
    pub tailscale: TailscaleStatus,
    pub wireguard: WireguardStatus,
    pub kill_switch: bool,
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn service_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", unit])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false)
}

fn iface_exists(name: &str) -> bool {
    std::path::Path::new("/sys/class/net").join(name).exists()
}

pub fn installed() -> InstalledStatus {
    InstalledStatus {
        tailscale: which("tailscale"),
        tailscaled_active: service_active("tailscaled"),
        wg: which("wg"),
        wg_quick: which("wg-quick"),
    }
}

pub fn tunnel_iface(cfg: &Config) -> Option<&'static str> {
    match cfg.vpn.backend.as_str() {
        "tailscale" => Some("tailscale0"),
        "wireguard" => Some("wg0"),
        _ => None,
    }
}

#[derive(Deserialize, Default)]
struct TsStatus {
    #[serde(default, rename = "BackendState")]
    backend_state: String,
    #[serde(default, rename = "Self")]
    self_: TsSelf,
}

#[derive(Deserialize, Default)]
struct TsSelf {
    #[serde(default, rename = "HostName")]
    host_name: String,
    #[serde(default, rename = "TailscaleIPs")]
    tailscale_ips: Vec<String>,
}

fn tailscale_status() -> TailscaleStatus {
    let mut ts = TailscaleStatus::default();
    match run("tailscale", &["status", "--json"]) {
        Ok(out) => {
            if let Ok(parsed) = serde_json::from_str::<TsStatus>(&out) {
                ts.state = parsed.backend_state;
                ts.hostname = parsed.self_.host_name;
                ts.ip = parsed
                    .self_
                    .tailscale_ips
                    .into_iter()
                    .find(|i| i.starts_with("100."))
                    .unwrap_or_default();
            }
        }
        Err(_) => {
            ts.state = "Stopped".into();
        }
    }
    ts
}

fn wireguard_status() -> WireguardStatus {
    let mut ws = WireguardStatus::default();
    ws.iface_up = iface_exists("wg0");
    if let Ok(dump) = run("wg", &["show", "wg0", "dump"]) {
        for line in dump.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 7 {
                ws.endpoint = f[2].to_string();
                if let Ok(hs) = f[4].parse::<u64>() {
                    if hs > 0 {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        ws.handshake_age_secs = Some(now.saturating_sub(hs));
                    }
                }
                break;
            }
        }
    }
    ws
}

pub fn status(cfg: &Config) -> VpnStatus {
    let inst = installed();
    let tailscale = if inst.tailscale {
        tailscale_status()
    } else {
        TailscaleStatus::default()
    };
    let wireguard = if inst.wg {
        wireguard_status()
    } else {
        WireguardStatus::default()
    };
    VpnStatus {
        backend: cfg.vpn.backend.clone(),
        role: cfg.vpn.role.clone(),
        installed: inst,
        tailscale,
        wireguard,
        kill_switch: kill_switch_active(),
    }
}

fn kill_switch_active() -> bool {
    run("nft", &["list", "chain", "inet", "travel-vpn", "killswitch"])
        .map(|out| out.contains("drop"))
        .unwrap_or(false)
}

fn wg_server_subnet(cfg: &Config) -> String {
    let (ip, prefix) = cfg
        .vpn
        .wg_address
        .split_once('/')
        .unwrap_or(("10.0.0.2", "24"));
    let mut oct: Vec<&str> = ip.split('.').collect();
    if oct.len() == 4 {
        oct[3] = "0";
    }
    format!("{}/{}", oct.join("."), prefix)
}

fn generate_table(cfg: &Config) -> String {
    let ap = cfg.ap_interface.as_str();
    let wg_port = cfg.vpn.wg_listen_port;
    let wg_subnet = wg_server_subnet(cfg);
    format!(
        r#"#!/usr/sbin/nft -f

table inet travel-vpn {{
    chain postrouting {{
        type nat hook postrouting priority srcnat; policy accept;
        oifname "wg0" masquerade
        oifname "tailscale0" masquerade
    }}

    chain forward {{
        type filter hook forward priority filter; policy accept;
        iifname "{ap}" oifname "wg0" accept
        iifname "{ap}" oifname "tailscale0" accept
        iifname "wg0" oifname "{ap}" ct state related,established accept
        iifname "tailscale0" oifname "{ap}" ct state related,established accept
        ip saddr {wg_subnet} accept
    }}

    chain input {{
        type filter hook input priority filter; policy accept;
        udp dport {wg_port} accept
    }}

    chain killswitch {{
        type filter hook forward priority filter + 1; policy accept;
    }}
}}
"#,
        ap = ap,
        wg_port = wg_port,
        wg_subnet = wg_subnet,
    )
}

pub fn nft_assert(cfg: &Config) -> Result<(), String> {
    fs::write(VPN_NFT_FILE, generate_table(cfg)).map_err(|e| format!("Write vpn nft: {e}"))?;
    run("nft", &["-f", VPN_NFT_FILE]).map(|_| ())
}

pub fn nft_teardown() {
    let _ = run("nft", &["delete", "table", "inet", "travel-vpn"]);
}

pub fn kill_switch_set(cfg: &Config) -> Result<(), String> {
    let tun = tunnel_iface(cfg).ok_or("no tunnel interface configured")?;
    run("nft", &["flush", "chain", "inet", "travel-vpn", "killswitch"])?;
    let args = vec![
        "add",
        "rule",
        "inet",
        "travel-vpn",
        "killswitch",
        "iifname",
        cfg.ap_interface.as_str(),
        "oifname",
        "!=",
        tun,
        "drop",
    ];
    run("nft", &args).map(|_| ())
}

pub fn kill_switch_clear() -> Result<(), String> {
    run("nft", &["flush", "chain", "inet", "travel-vpn", "killswitch"]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> Config {
        Config {
            vpn: crate::config::VpnConfig {
                backend: "wireguard".into(),
                role: "travel".into(),
                wg_listen_port: 51820,
                wg_address: "10.0.0.2/24".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn generate_table_contains_expected_rules() {
        let t = generate_table(&test_cfg());
        assert!(t.contains("table inet travel-vpn"));
        assert!(t.contains("oifname \"wg0\" masquerade"));
        assert!(t.contains("oifname \"tailscale0\" masquerade"));
        assert!(t.contains("udp dport 51820 accept"));
        assert!(t.contains("iifname \"wlan1\" oifname \"wg0\" accept"));
        assert!(t.contains("chain killswitch"));
    }

    #[test]
    fn wg_server_subnet_from_address() {
        assert_eq!(wg_server_subnet(&test_cfg()), "10.0.0.0/24");
    }

    #[test]
    fn tunnel_iface_by_backend() {
        let mut cfg = test_cfg();
        assert_eq!(tunnel_iface(&cfg), Some("wg0"));
        cfg.vpn.backend = "tailscale".into();
        assert_eq!(tunnel_iface(&cfg), Some("tailscale0"));
        cfg.vpn.backend = "none".into();
        assert_eq!(tunnel_iface(&cfg), None);
    }
}
