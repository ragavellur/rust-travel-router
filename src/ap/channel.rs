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
            let freq: u32 = freq_str
                .trim()
                .parse::<f32>()
                .ok()?
                .round() as u32;
            return freq_to_channel_and_band(freq);
        }
    }
    None
}

/// Return the channel a given interface is actually operating on,
/// parsed from `iw dev <iface> info`.
pub fn detect_ap_channel(ap_iface: &str) -> Option<u8> {
    let output = Command::new("iw")
        .args(["dev", ap_iface, "info"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("channel") {
            let rest = rest.trim_start();
            let num = rest.split_whitespace().next()?;
            return num.parse::<u8>().ok();
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

/// Scan the given interface for nearby APs and count how many overlap each 2.4GHz channel.
/// Returns the least congested channel among 1, 6, 11 (non-overlapping), selecting the
/// one with fewest networks near it. Falls back to 6 if the scan fails.
pub fn scan_least_congested_channel(scan_iface: &str) -> u8 {
    let output = Command::new("iw")
        .args(["dev", scan_iface, "scan"])
        .output();
    let text = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => {
            tracing::warn!("WiFi scan on {scan_iface} failed, using channel 6");
            return 6;
        }
    };

    let mut channel_load: std::collections::HashMap<u8, u32> = std::collections::HashMap::new();
    let mut current_freq: Option<u32> = None;
    let mut current_signal: Option<i32> = None;
    let mut in_bss = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("BSS ") {
            in_bss = true;
            current_freq = None;
            current_signal = None;
            continue;
        }
        if !in_bss {
            continue;
        }
        if let Some(f) = line.strip_prefix("freq:") {
            current_freq = f.trim().parse::<f32>().ok().map(|v| v.round() as u32);
        } else if let Some(s) = line.strip_prefix("signal:") {
            let parts: Vec<&str> = s.trim().split_whitespace().collect();
            if let Some(v) = parts.first() {
                current_signal = v.parse::<f32>().ok().map(|v| v.round() as i32);
            }
        } else if line.strip_prefix("SSID:").is_some() {
            if let Some(freq) = current_freq {
                if (2412..=2472).contains(&freq) {
                    let ch = ((freq - 2412) / 5 + 1) as u8;
                    // Weight: count every overlapping channel, but only count
                    // networks with reasonable signal (-90 dBm or better).
                    if current_signal.unwrap_or(-100) >= -90 {
                        *channel_load.entry(ch).or_insert(0) += 1;
                    }
                }
            }
            in_bss = false;
        }
    }

    let candidates = [1u8, 6u8, 11u8];
    let best = candidates.iter().copied().min_by_key(|&ch| {
        // Load of a candidate channel = sum of networks on overlapping channels
        let mut load = 0u32;
        for (c, n) in channel_load.iter() {
            let overlap = if ch <= 4 {
                *c <= 6
            } else if ch >= 9 {
                *c >= 6
            } else {
                (*c as i32 - ch as i32).abs() <= 2
            };
            if overlap {
                load += n;
            }
        }
        load
    });

    let chosen = best.unwrap_or(6);
    tracing::info!(
        "Least congested 2.4GHz channel: {chosen} (loads: {:?})",
        channel_load
    );
    chosen
}

pub fn resolve_ap_channel(cfg_ap_channel: u8, cfg_ap_band: &str, sta_iface: &str, scan_iface: &str) -> (u8, String) {
    if cfg_ap_channel != 0 {
        return (cfg_ap_channel, cfg_ap_band.to_string());
    }
    if let Some((ch, band)) = detect_sta_channel(sta_iface) {
        tracing::info!("Auto-detected STA channel {ch} ({band}) from {sta_iface}");
        return (ch, band);
    }
    let ch = scan_least_congested_channel(scan_iface);
    (ch, "bg".into())
}
