use ipnetwork::Ipv4Network;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_ap_ssid")]
    pub ap_ssid: String,

    #[serde(default)]
    pub ap_password: String,

    #[serde(default = "default_ap_ip")]
    pub ap_ip: String,

    #[serde(default = "default_ap_netmask")]
    pub ap_netmask: String,

    #[serde(default = "default_ap_channel")]
    pub ap_channel: u8,

    #[serde(default = "default_dhcp_start")]
    pub dhcp_start: String,

    #[serde(default = "default_dhcp_end")]
    pub dhcp_end: String,

    #[serde(default = "default_dhcp_lease_hours")]
    pub dhcp_lease_hours: u32,

    #[serde(default = "default_hostname")]
    pub hostname: String,

    #[serde(default = "default_sta_interface")]
    pub sta_interface: String,

    #[serde(default = "default_ap_interface")]
    pub ap_interface: String,

    #[serde(default)]
    pub sta_ssid: String,

    #[serde(default)]
    pub sta_password: String,

    #[serde(default)]
    pub wifi_backend: String,

    #[serde(default)]
    pub web_password: String,

    #[serde(default = "default_ap_band")]
    pub ap_band: String,

    #[serde(default = "default_power_mode")]
    pub power_mode: String,

    #[serde(default)]
    pub rootfs_readonly: bool,

    #[serde(default)]
    pub vpn: VpnConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnConfig {
    #[serde(default = "default_vpn_backend")]
    pub backend: String,

    #[serde(default = "default_vpn_role")]
    pub role: String,

    #[serde(default)]
    pub ts_auth_key: String,

    #[serde(default)]
    pub ts_hostname: String,

    #[serde(default)]
    pub ts_exit_node: String,

    #[serde(default)]
    pub ts_advertise_routes: String,

    #[serde(default = "default_true")]
    pub ts_route_all: bool,

    #[serde(default = "default_true")]
    pub ts_allow_lan: bool,

    #[serde(default)]
    pub wg_private_key: String,

    #[serde(default)]
    pub wg_peer_public_key: String,

    #[serde(default)]
    pub wg_endpoint: String,

    #[serde(default = "default_wg_address")]
    pub wg_address: String,

    #[serde(default)]
    pub wg_dns: String,

    #[serde(default)]
    pub wg_preshared_key: String,

    #[serde(default = "default_wg_keepalive")]
    pub wg_keepalive: u32,

    #[serde(default = "default_true")]
    pub wg_route_all: bool,

    #[serde(default = "default_wg_listen_port")]
    pub wg_listen_port: u16,

    #[serde(default)]
    pub wg_server_private_key: String,

    #[serde(default)]
    pub wg_server_public_key: String,

    #[serde(default)]
    pub wg_peers: Vec<WgPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WgPeer {
    pub name: String,
    pub public_key: String,
    pub tunnel_ip: String,
}

fn default_ap_ssid() -> String { "RagaNeoAir".into() }
fn default_ap_ip() -> String { "192.168.4.1".into() }
fn default_ap_netmask() -> String { "255.255.255.0".into() }
fn default_ap_channel() -> u8 { 0 }
fn default_ap_band() -> String { "bg".into() }
fn default_dhcp_start() -> String { "192.168.4.10".into() }
fn default_dhcp_end() -> String { "192.168.4.250".into() }
fn default_dhcp_lease_hours() -> u32 { 24 }
fn default_hostname() -> String { "travel-router".into() }
fn default_sta_interface() -> String { "wlan0".into() }
fn default_ap_interface() -> String { "wlan1".into() }
fn default_power_mode() -> String { "performance".into() }
fn default_vpn_backend() -> String { "none".into() }
fn default_vpn_role() -> String { "travel".into() }
fn default_true() -> bool { true }
fn default_wg_address() -> String { "10.0.0.2/24".into() }
fn default_wg_keepalive() -> u32 { 25 }
fn default_wg_listen_port() -> u16 { 51820 }

impl Default for VpnConfig {
    fn default() -> Self {
        Self {
            backend: default_vpn_backend(),
            role: default_vpn_role(),
            ts_auth_key: String::new(),
            ts_hostname: String::new(),
            ts_exit_node: String::new(),
            ts_advertise_routes: String::new(),
            ts_route_all: true,
            ts_allow_lan: true,
            wg_private_key: String::new(),
            wg_peer_public_key: String::new(),
            wg_endpoint: String::new(),
            wg_address: default_wg_address(),
            wg_dns: String::new(),
            wg_preshared_key: String::new(),
            wg_keepalive: default_wg_keepalive(),
            wg_route_all: true,
            wg_listen_port: default_wg_listen_port(),
            wg_server_private_key: String::new(),
            wg_server_public_key: String::new(),
            wg_peers: Vec::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ap_ssid: default_ap_ssid(),
            ap_password: String::new(),
            ap_ip: default_ap_ip(),
            ap_netmask: default_ap_netmask(),
            ap_channel: default_ap_channel(),
            dhcp_start: default_dhcp_start(),
            dhcp_end: default_dhcp_end(),
            dhcp_lease_hours: default_dhcp_lease_hours(),
            hostname: default_hostname(),
            sta_interface: default_sta_interface(),
            ap_interface: default_ap_interface(),
            sta_ssid: String::new(),
            sta_password: String::new(),
            wifi_backend: String::new(),
            web_password: String::new(),
            ap_band: default_ap_band(),
            power_mode: default_power_mode(),
            rootfs_readonly: false,
            vpn: VpnConfig::default(),
        }
    }
}

impl Config {
    pub fn ap_network(&self) -> Result<Ipv4Network, String> {
        if self.ap_ip.contains('/') {
            self.ap_ip.parse()
                .map_err(|e| format!("Invalid AP IP: {e}"))
        } else {
            Ipv4Network::with_netmask(
                self.ap_ip.parse().map_err(|e| format!("Invalid AP IP: {e}"))?,
                self.ap_netmask.parse().map_err(|e| format!("Invalid netmask: {e}"))?,
            )
            .map_err(|e| format!("Invalid network: {e}"))
        }
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.ap_ssid.is_empty() {
            errors.push("AP SSID is required".into());
        }
        if self.ap_ssid.len() > 32 {
            errors.push("AP SSID must be 32 chars or less".into());
        }
        if !self.ap_password.is_empty() && self.ap_password.len() < 8 {
            errors.push("AP password must be at least 8 characters".into());
        }
        if !["bg", "a", "auto"].contains(&self.ap_band.as_str()) {
            errors.push("ap_band must be 'bg', 'a', or 'auto'".into());
        }
        if self.ap_band == "a" {
            let valid = (36..=64).contains(&self.ap_channel)
                || (100..=144).contains(&self.ap_channel)
                || (149..=165).contains(&self.ap_channel)
                || self.ap_channel == 0;
            if !valid {
                errors.push("5GHz channel must be 36-64, 100-144, 149-165, or 0 (auto)".into());
            }
        } else if self.ap_band == "bg" {
            if self.ap_channel != 0 && (self.ap_channel < 1 || self.ap_channel > 13) {
                errors.push("2.4GHz channel must be 1-13 or 0 (auto)".into());
            }
        }
        // "auto" band: skip channel check (driver auto-selects)
        if let Err(e) = self.ap_network() {
            errors.push(format!("Invalid AP IP/netmask: {e}"));
        }
        if !["performance", "powersave"].contains(&self.power_mode.as_str()) {
            errors.push("power_mode must be 'performance' or 'powersave'".into());
        }
        if let Err(v) = self.vpn.validate() {
            errors.extend(v);
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

impl VpnConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if !["none", "tailscale", "wireguard"].contains(&self.backend.as_str()) {
            errors.push("VPN backend must be 'none', 'tailscale', or 'wireguard'".into());
        }
        if !["home", "travel"].contains(&self.role.as_str()) {
            errors.push("VPN role must be 'home' or 'travel'".into());
        }
        match self.backend.as_str() {
            "tailscale" => {
                if self.ts_auth_key.is_empty() {
                    errors.push("Tailscale auth key is required (Tailscale Admin Console → Settings → Keys → Generate auth key)".into());
                }
                if self.role == "travel" && self.ts_exit_node.is_empty() {
                    errors.push("Tailscale home node is required for the travel box (the exit node that stays at home)".into());
                }
            }
            "wireguard" => {
                if self.role == "travel" {
                    if self.wg_private_key.is_empty() {
                        errors.push("WireGuard private key is required (press 'Generate my key' to make one)".into());
                    }
                    if self.wg_peer_public_key.is_empty() {
                        errors.push("WireGuard peer public key is required (copy it from the home side)".into());
                    }
                    if self.wg_endpoint.is_empty() {
                        errors.push("WireGuard endpoint is required, e.g. myhome.example.com:51820".into());
                    }
                    if self.wg_address.parse::<Ipv4Network>().is_err() {
                        errors.push("WireGuard tunnel address must be a valid CIDR, e.g. 10.0.0.2/24".into());
                    }
                } else {
                    if self.wg_server_private_key.is_empty() {
                        errors.push("WireGuard server private key is required (press 'Generate server key')".into());
                    }
                    if self.wg_listen_port == 0 {
                        errors.push("WireGuard listen port must be 1-65535".into());
                    }
                    if self.wg_endpoint.is_empty() {
                        errors.push("Home endpoint is required — the public address travel devices use to reach this box, e.g. myhome.example.com:51820".into());
                    }
                    let mut seen = std::collections::HashSet::new();
                    for p in &self.wg_peers {
                        if p.public_key.is_empty() {
                            errors.push(format!("Peer '{}' is missing its public key", p.name));
                        }
                        if p.tunnel_ip.parse::<std::net::Ipv4Addr>().is_err() {
                            errors.push(format!("Peer '{}' has an invalid tunnel IP: {}", p.name, p.tunnel_ip));
                        }
                        if !seen.insert(p.tunnel_ip.clone()) {
                            errors.push(format!("Duplicate tunnel IP among peers: {}", p.tunnel_ip));
                        }
                    }
                }
            }
            _ => {}
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

pub fn load(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    if !path.exists() {
        tracing::info!("Config not found, using defaults");
        return Ok(Config::default());
    }
    let data = fs::read_to_string(path)?;
    let cfg: Config = serde_json::from_str(&data)?;
    Ok(cfg)
}

pub fn save(path: &Path, cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(cfg)?;
    fs::write(&tmp, &data)?;
    fs::rename(&tmp, path)?;
    crate::system::remount::persist_config();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_without_vpn_loads() {
        let old = r#"{"ap_ssid":"Travel-Net","ap_password":"travelnet"}"#;
        let cfg: Config = serde_json::from_str(old).unwrap();
        assert_eq!(cfg.vpn.backend, "none");
        assert_eq!(cfg.vpn.role, "travel");
        assert_eq!(cfg.vpn.wg_listen_port, 51820);
        assert!(cfg.vpn.wg_route_all);
        assert!(cfg.vpn.ts_allow_lan);
    }

    #[test]
    fn vpn_default_is_disabled() {
        let cfg = Config::default();
        assert_eq!(cfg.vpn.backend, "none");
        assert!(cfg.vpn.wg_peers.is_empty());
        assert!(cfg.vpn.validate().is_ok());
    }

    #[test]
    fn vpn_validation_catches_missing_tailscale_fields() {
        let cfg = Config {
            vpn: VpnConfig {
                backend: "tailscale".into(),
                role: "travel".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let errs = cfg.vpn.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("auth key")));
        assert!(errs.iter().any(|e| e.contains("home node")));
    }

    #[test]
    fn vpn_validation_catches_missing_wireguard_client_fields() {
        let cfg = Config {
            vpn: VpnConfig {
                backend: "wireguard".into(),
                role: "travel".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let errs = cfg.vpn.validate().unwrap_err();
        assert!(errs.iter().any(|e| e.contains("private key")));
        assert!(errs.iter().any(|e| e.contains("public key")));
        assert!(errs.iter().any(|e| e.contains("endpoint")));
    }

    #[test]
    fn vpn_serialization_roundtrip() {
        let cfg = Config {
            vpn: VpnConfig {
                backend: "wireguard".into(),
                role: "home".into(),
                wg_listen_port: 51820,
                wg_server_private_key: "SRVKEY".into(),
                wg_peers: vec![WgPeer {
                    name: "travel".into(),
                    public_key: "PEERKEY".into(),
                    tunnel_ip: "10.0.0.2".into(),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.vpn.backend, "wireguard");
        assert_eq!(back.vpn.wg_peers[0].name, "travel");
        assert_eq!(back.vpn.wg_peers[0].tunnel_ip, "10.0.0.2");
    }

    #[test]
    fn ap_channel_zero_is_valid_for_bg() {
        let cfg = Config { ap_channel: 0, ap_band: "bg".into(), ..Default::default() };
        assert!(cfg.validate().is_ok(), "ap_channel=0 should be valid for bg band");
    }

    #[test]
    fn ap_channel_zero_is_valid_for_a() {
        let cfg = Config { ap_channel: 0, ap_band: "a".into(), ..Default::default() };
        assert!(cfg.validate().is_ok(), "ap_channel=0 should be valid for a band");
    }

    #[test]
    fn ap_channel_default_is_zero() {
        let cfg = Config::default();
        assert_eq!(cfg.ap_channel, 0);
    }

    #[test]
    fn freq_to_channel_2ghz() {
        assert_eq!(crate::ap::channel::freq_to_channel_and_band(2412), Some((1, "bg".into())));
        assert_eq!(crate::ap::channel::freq_to_channel_and_band(2437), Some((6, "bg".into())));
        assert_eq!(crate::ap::channel::freq_to_channel_and_band(2462), Some((11, "bg".into())));
    }

    #[test]
    fn freq_to_channel_5ghz() {
        assert_eq!(crate::ap::channel::freq_to_channel_and_band(5180), Some((36, "a".into())));
        assert_eq!(crate::ap::channel::freq_to_channel_and_band(5745), Some((149, "a".into())));
    }

    #[test]
    fn freq_to_channel_unknown() {
        assert_eq!(crate::ap::channel::freq_to_channel_and_band(900), None);
    }
}
