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
}

fn default_ap_ssid() -> String { "RagaNeoAir".into() }
fn default_ap_ip() -> String { "192.168.4.1".into() }
fn default_ap_netmask() -> String { "255.255.255.0".into() }
fn default_ap_channel() -> u8 { 4 }
fn default_ap_band() -> String { "bg".into() }
fn default_dhcp_start() -> String { "192.168.4.10".into() }
fn default_dhcp_end() -> String { "192.168.4.250".into() }
fn default_dhcp_lease_hours() -> u32 { 24 }
fn default_hostname() -> String { "travel-router".into() }
fn default_sta_interface() -> String { "wlan0".into() }
fn default_ap_interface() -> String { "wlan1".into() }

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
        }
    }
}

impl Config {
    pub fn ap_network(&self) -> Result<Ipv4Network, String> {
        if self.ap_ip.contains('/') {
            self.ap_ip.parse()
                .map_err(|e| format!("Invalid AP IP: {e}"))
        } else {
            Ipv4Network::new(
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
            if self.ap_channel < 1 || self.ap_channel > 13 {
                errors.push("2.4GHz channel must be 1-13".into());
            }
        }
        // "auto" band: skip channel check (driver auto-selects)
        if let Err(e) = self.ap_network() {
            errors.push(format!("Invalid AP IP/netmask: {e}"));
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
    Ok(())
}
