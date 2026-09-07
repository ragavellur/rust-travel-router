use crate::config::Config;
use crate::ap::channel;
use std::fs;
use std::process::Command;

const HOSTAPD_CONF: &str = "/etc/hostapd/travel-net.conf";
const HOSTAPD_PID: &str = "/run/travel-net/hostapd.pid";

pub async fn start_hostapd(cfg: &Config) -> Result<(), String> {
    stop_hostapd().await;
    fs::create_dir_all("/run/travel-net").ok();

    // Detect the STA channel with a few short retries before falling back to a
    // scan. On a single-radio device the AP MUST run on the STA channel when a
    // STA is connected: hostapd on a different channel "starts" but the firmware
    // keeps the radio on the STA channel, producing beacons that advertise the
    // wrong channel. A momentarily reconnecting STA at boot would otherwise
    // cause this.
    let mut sta_channel: Option<u8> = None;
    for _ in 0..5 {
        if let Some((c, _)) = channel::detect_sta_channel(&cfg.sta_interface) {
            sta_channel = Some(c);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }

    // Only scan (which may briefly disrupt the radio) when no STA channel was found.
    let scan_channel = match sta_channel {
        Some(_) => {
            tracing::info!("Using STA channel for AP; skipping congestion scan");
            0
        }
        None => channel::scan_least_congested_channel(&cfg.sta_interface),
    };

    // Candidate channels to try in order:
    // 1. Explicitly configured channel
    // 2. STA channel (single-radio: AP must match STA)
    // 3. Least congested scanned channel
    // 4. Safe defaults
    let mut candidates: Vec<u8> = Vec::new();
    if cfg.ap_channel != 0 {
        candidates.push(cfg.ap_channel);
    }
    if let Some(c) = sta_channel {
        if !candidates.contains(&c) {
            candidates.push(c);
        }
    }
    if scan_channel != 0 && !candidates.contains(&scan_channel) {
        candidates.push(scan_channel);
    }
    for c in [6u8, 1u8, 11u8] {
        if !candidates.contains(&c) {
            candidates.push(c);
        }
    }

    let mut last_err = String::new();
    for (i, ch) in candidates.iter().enumerate() {
        tracing::info!("hostapd attempt {}/{} channel {ch}", i + 1, candidates.len());
        // Recreate the interface fresh for every attempt. A leftover interface
        // from a previous service instance (or a failed hostapd start) is stuck
        // in DISABLED state and any reuse fails.
        let _ = std::process::Command::new("iw")
            .args(["dev", &cfg.ap_interface, "del"])
            .output();
        if let Err(e) = crate::ap::interface::create_ap_interface(cfg).await {
            tracing::warn!("Failed to recreate AP interface for channel {ch}: {e}");
        }
        if let Err(e) = crate::ap::assign_ap_ip(cfg) {
            tracing::warn!("Failed to reassign AP IP for channel {ch}: {e}");
        }
        if let Err(e) = generate_conf_with(cfg, *ch) {
            last_err = e;
            continue;
        }
        match spawn_hostapd(cfg).await {
            Ok(()) => {
                tracing::info!("hostapd started on channel {ch} (SSID: {})", cfg.ap_ssid);
                return Ok(());
            }
            Err(e) => {
                last_err = e;
                stop_hostapd().await;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Err(format!("hostapd failed on all candidate channels: {last_err}"))
}

pub async fn spawn_hostapd(cfg: &Config) -> Result<(), String> {
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

pub fn generate_conf_with(cfg: &Config, channel: u8) -> Result<(), String> {
    write_conf(cfg, channel, "bg")
}

fn write_conf(cfg: &Config, channel: u8, band: &str) -> Result<(), String> {
    let hw_mode = match band {
        "a" => "a",
        _ => "g",
    };

    let mut extra = String::from("ieee80211n=1\nwmm_enabled=1\nuapsd_advertisement_enabled=0\n");

    if hw_mode == "a" {
        extra.push_str("ht_capab=[HT40+][HT40-][SHORT-GI-20][SHORT-GI-40][RX-STBC1]\n");
        extra.push_str("ieee80211ac=1\n");
        extra.push_str("vht_capab=[MAX-MPDU-7991][SHORT-GI-80][RX-STBC-1][MAX-A-MPDU-LEN-EXP-3]\n");
        extra.push_str("vht_oper_chwidth=1\n");
        let seg0 = if channel <= 48 { 42 }
            else if channel <= 64 { 58 }
            else if channel <= 112 { 106 }
            else if channel <= 128 { 122 }
            else if channel <= 144 { 138 }
            else { 155 };
        extra.push_str(&format!("vht_oper_centr_freq_seg0_idx={seg0}\n"));
    } else {
        // HT20 only, no LDPC. HT40 makes brcmfmac reject valid channels
        // ("secondary_channel not found in channel list") unless it is exactly
        // the STA channel, and [LDPC] is not supported by the driver at all
        // ("Driver does not support configured HT capability [LDPC]").
        extra.push_str("ht_capab=[HT20][SHORT-GI-20]\n");
    }

    tracing::info!("hostapd channel: {channel} (band: {hw_mode})");

    let conf = format!(
        r#"interface={iface}
driver=nl80211
ssid={ssid}
hw_mode={hw_mode}
channel={channel}
{extra}macaddr_acl=0
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
        hw_mode = hw_mode,
        channel = channel,
        extra = extra,
        password = if cfg.ap_password.is_empty() { "travel-net".into() } else { cfg.ap_password.clone() },
    );

    crate::system::remount::safe_write(std::path::Path::new(HOSTAPD_CONF), &conf)
}
