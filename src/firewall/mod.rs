use crate::config::Config;
use std::process::Command;

pub async fn apply_ruleset(cfg: &Config) -> Result<(), String> {
    let rules = generate_ruleset(cfg);
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

fn generate_ruleset(cfg: &Config) -> String {
    format!(
        r#"#!/usr/sbin/nft -f

flush ruleset

table inet nat {{
    chain postrouting {{
        type nat hook postrouting priority srcnat; policy accept;
        oifname "{sta_iface}" masquerade
    }}

    chain prerouting {{
        type nat hook prerouting priority dstnat; policy accept;
    }}
}}

table inet filter {{
    chain forward {{
        type filter hook forward priority filter; policy accept;
        iifname "{ap_iface}" oifname "{sta_iface}" accept
        iifname "{sta_iface}" oifname "{ap_iface}" ct state related,established accept
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
        sta_iface = cfg.sta_interface,
    )
}
