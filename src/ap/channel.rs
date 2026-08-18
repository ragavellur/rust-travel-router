use std::process::Command;

pub fn detect_sta_channel(sta_iface: &str) -> Option<(u8, String)> {
    let output = Command::new("iw")
        .args(["dev", sta_iface, "link"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(freq_str) = line.strip_prefix("freq:") {
            let freq: u32 = freq_str.trim().parse().ok()?;
            return freq_to_channel_and_band(freq);
        }
    }
    None
}

pub fn freq_to_channel_and_band(freq: u32) -> Option<(u8, String)> {
    match freq {
        2412..=2472 => {
            let ch = ((freq - 2412) / 5 + 1) as u8;
            Some((ch, "bg".into()))
        }
        2484 => Some((14, "bg".into())),
        5180..=5825 => {
            let ch = match freq {
                5180 => 36, 5200 => 40, 5220 => 44, 5240 => 48,
                5260 => 52, 5280 => 56, 5300 => 60, 5320 => 64,
                5500 => 100, 5520 => 104, 5540 => 108, 5560 => 112,
                5580 => 116, 5600 => 120, 5620 => 124, 5640 => 128,
                5660 => 132, 5680 => 136, 5700 => 140,
                5745 => 149, 5765 => 153, 5785 => 157, 5805 => 161,
                5825 => 165,
                _ => return None,
            };
            Some((ch, "a".into()))
        }
        _ => None,
    }
}

pub fn resolve_ap_channel(cfg_ap_channel: u8, cfg_ap_band: &str, sta_iface: &str) -> (u8, String) {
    if cfg_ap_channel != 0 {
        return (cfg_ap_channel, cfg_ap_band.to_string());
    }
    if sta_iface.is_empty() {
        return (0, cfg_ap_band.to_string());
    }
    if let Some((ch, band)) = detect_sta_channel(sta_iface) {
        tracing::info!("Auto-detected STA channel {ch} ({band}) from {sta_iface}");
        return (ch, band);
    }
    tracing::warn!("Could not detect STA channel from {sta_iface}, using auto (channel 0)");
    (0, cfg_ap_band.to_string())
}
