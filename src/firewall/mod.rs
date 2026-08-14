use crate::config::Config;
use std::process::Command;

/// Detect the actual uplink interface by reading the default route.
/// Picks the lowest-metric default route from `ip route show default`.
/// Falls back to `cfg.sta_interface` if no default route exists.
pub fn detect_uplink(cfg: &Config) -> String {
    if let Ok(out) = Command::new("ip").args(["route", "show", "default"]).output() {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = stdout.lines().next() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for (i, part) in parts.iter().enumerate() {
                    if *part == "dev" && i + 1 < parts.len() {
                        let iface = parts[i + 1];
                        tracing::debug!("Detected uplink: {iface}");
                        return iface.to_string();
                    }
                }
            }
        }
    }
    tracing::debug!("No default route found, falling back to {}", cfg.sta_interface);
    cfg.sta_interface.clone()
}

pub async fn apply_ruleset(cfg: &Config) -> Result<(), String> {
    let uplink = detect_uplink(cfg);
    let rules = generate_ruleset(cfg, &uplink);
    let nft_file = "/etc/travel-net/travel-net.nft";

    std::fs::write(nft_file, &rules).map_err(|e| format!("Write nftables rules: {e}"))?;

    // Enable IP forwarding
    let fw = Command::new("sysctl")
        .args(["-w", "net.ipv4.ip_forward=1"])
        .output()
        .map_err(|e| format!("sysctl error: {e}"))?;

    if !fw.status.success() {
        let stderr = String::from_utf8_lossy(&fw.stderr);
        tracing::warn!("Failed to set ip_forward=1: {stderr}");
    }

    let out = Command::new("nft")
        .args(["-f", nft_file])
        .output()
        .map_err(|e| format!("nftables apply error: {e}"))?;

    if out.status.success() {
        tracing::info!("nftables rules applied");
        // `flush ruleset` wiped the VPN table — re-assert it so the kill
        // switch, tunnel NAT and UDP accept survive any ruleset re-apply.
        if cfg.vpn.backend != "none" {
            if let Err(e) = crate::vpn::reassert_rules(cfg) {
                tracing::warn!("Failed to re-assert VPN nft rules: {e}");
            }
        }
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(format!("nftables apply failed: {stderr}"))
    }
}

pub async fn flush_ruleset() -> Result<(), String> {
    Command::new("nft")
        .args(["flush", "ruleset"])
        .output()
        .map(|_| ())
        .map_err(|e| format!("nftables flush error: {e}"))
}

pub fn apply_performance_tuning(cfg: &Config) {
    let is_perf = cfg.power_mode == "performance";
    let uplink = detect_uplink(cfg);

    // CPU governor: performance or ondemand
    let gov = if is_perf { "performance" } else { "ondemand" };
    for i in 0..8 {
        let path = format!("/sys/devices/system/cpu/cpu{i}/cpufreq/scaling_governor");
        if let Ok(_) = std::fs::write(&path, gov) {
            tracing::debug!("cpu{i} governor → {gov}");
        }
    }

    // RPS: distribute IRQs across all CPUs (performance) or CPU 0 only (powersave)
    let rps = if is_perf { "ff" } else { "0" };
    let ifaces = [uplink.as_str(), cfg.ap_interface.as_str()];
    for iface in ifaces {
        let path = format!("/sys/class/net/{iface}/queues/rx-0/rps_cpus");
        if let Ok(_) = std::fs::write(&path, rps) {
            tracing::debug!("{iface} RPS → {rps}");
        }
    }

    // Disable WiFi power save on uplink if it is a wireless interface
    let _ = Command::new("iw")
        .args(["dev", &uplink, "set", "power_save", "off"])
        .output();

    if is_perf {
        // AIC8800 driver tuning for performance mode
        let kernel_params: &[(&str, &str)] = &[
            ("/sys/module/aic8800_fdrv/parameters/tx_aggr_counter", "64"),
            ("/sys/module/aic8800_fdrv/parameters/reorder_timeout", "200"),
            ("/sys/module/aic8800_fdrv/parameters/tx_lft", "500"),
        ];
        for (path, val) in kernel_params {
            if let Ok(_) = std::fs::write(path, val) {
                tracing::debug!("aic8800 {path} → {val}");
            }
        }
    }

    // Create modprobe config for next boot (aic8800 power save off)
    let modprobe_conf = "options aic8800_fdrv ps_on=0 ap_uapsd_on=0\n";
    let _ = std::fs::write("/etc/modprobe.d/aic8800-travel-net.conf", modprobe_conf);

    tracing::info!("Performance tuning applied: mode={}", cfg.power_mode);
}

fn generate_ruleset(cfg: &Config, uplink_iface: &str) -> String {
    format!(
        r#"#!/usr/sbin/nft -f

flush ruleset

table inet nat {{
    chain postrouting {{
        type nat hook postrouting priority srcnat; policy accept;
        oifname "{uplink}" masquerade
    }}

    chain prerouting {{
        type nat hook prerouting priority dstnat; policy accept;
    }}
}}

table inet filter {{
    chain forward {{
        type filter hook forward priority filter; policy accept;
        iifname "{ap_iface}" oifname "{uplink}" accept
        iifname "{uplink}" oifname "{ap_iface}" ct state related,established accept
    }}

    chain input {{
        type filter hook input priority filter; policy accept;
        iifname "{ap_iface}" tcp dport 80 accept
        iifname "{ap_iface}" udp dport 53 accept
        iifname "{ap_iface}" udp dport 67 accept
    }}
}}
"#,
        ap_iface = cfg.ap_interface,
        uplink = uplink_iface,
    )
}
