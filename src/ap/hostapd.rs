use crate::config::Config;
use std::fs;
use std::process::Command;

const HOSTAPD_CONF: &str = "/etc/hostapd/travel-net.conf";
const HOSTAPD_PID: &str = "/run/travel-net/hostapd.pid";

pub async fn start_hostapd(cfg: &Config) -> Result<(), String> {
    generate_conf(cfg)?;
    stop_hostapd().await;

    fs::create_dir_all("/run/travel-net").ok();

    let mut child = Command::new("hostapd")
        .args(["-B", "-P", HOSTAPD_PID, HOSTAPD_CONF])
        .spawn()
        .map_err(|e| format!("Failed to start hostapd: {e}"))?;

    let status = child.wait().map_err(|e| format!("hostapd wait: {e}"))?;
    if status.success() {
        tracing::info!("hostapd started (SSID: {})", cfg.ap_ssid);
        Ok(())
    } else {
        Err(format!("hostapd exited with {status:?}"))
    }
}

pub async fn stop_hostapd() {
    if let Ok(pid) = fs::read_to_string(HOSTAPD_PID) {
        if let Ok(pid) = pid.trim().parse::<i32>() {
            let _ = Command::new("kill").args([&pid.to_string()]).output();
        }
    }
    let _ = Command::new("pkill").args(["-x", "hostapd"]).output();
}

pub fn is_running() -> bool {
    Command::new("pgrep").args(["-x", "hostapd"]).output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn generate_conf(cfg: &Config) -> Result<(), String> {
    let conf = format!(
        r#"interface={iface}
driver=nl80211
ssid={ssid}
hw_mode=g
channel={channel}
wmm_enabled=1
macaddr_acl=0
auth_algs=1
ignore_broadcast_ssid=0
wpa=2
wpa_passphrase={password}
wpa_key_mgmt=WPA-PSK
wpa_pairwise=TKIP
rsn_pairwise=CCMP
ctrl_interface=/var/run/hostapd
"#,
        iface = cfg.ap_interface,
        ssid = cfg.ap_ssid,
        channel = cfg.ap_channel,
        password = if cfg.ap_password.is_empty() { "travel-net".into() } else { cfg.ap_password.clone() },
    );

    fs::write(HOSTAPD_CONF, &conf).map_err(|e| format!("Write hostapd.conf: {e}"))
}
