use serde::Serialize;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
pub struct DhcpClient {
    pub expiry: String,
    pub mac: String,
    pub ip: String,
    pub hostname: String,
    pub blocked: bool,
    pub active: bool,
}

fn read_leases() -> Vec<(String, String, String, String)> {
    let lease_paths = [
        "/var/lib/misc/dnsmasq.leases",
        "/var/lib/dnsmasq/dnsmasq.leases",
        "/var/lib/NetworkManager/dnsmasq-wlan1.leases",
    ];

    let data = lease_paths
        .iter()
        .find_map(|p| fs::read_to_string(p).ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_default();

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    data.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            let expiry: u64 = parts[0].parse().unwrap_or(0);
            if expiry < now {
                return None;
            }
            Some((
                parts[0].to_string(),
                parts[1].to_uppercase(),
                parts[2].to_string(),
                if parts[3] == "*" { String::new() } else { parts[3].to_string() },
            ))
        })
        .collect()
}

fn get_active_ips() -> Vec<String> {
    let output = Command::new("sh")
        .args(["-c", "cat /proc/net/nf_conntrack | grep -oP 'src=\\K[0-9.]+' | sort -u"])
        .output()
        .ok();
    match output {
        Some(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        }
        _ => vec![],
    }
}

fn get_blocked_ips() -> Vec<String> {
    let output = Command::new("nft")
        .args(["-j", "list", "set", "ip", "travel-net-block", "blocked"])
        .output()
        .ok();
    match output {
        Some(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            match serde_json::from_str::<serde_json::Value>(&stdout) {
                Ok(json) => {
                    if let Some(arr) = json["nftables"].as_array() {
                        let mut ips = Vec::new();
                        for entry in arr {
                            if let Some(elem) = entry["set"]["elem"].as_array() {
                                for v in elem {
                                    if let Some(ip) = v.as_str() {
                                        ips.push(ip.to_string());
                                    }
                                }
                            }
                        }
                        return ips;
                    }
                    vec![]
                }
                Err(_) => vec![],
            }
        }
        _ => vec![],
    }
}

fn ensure_block_table() {
    let check = Command::new("nft")
        .args(["list", "table", "ip", "travel-net-block"])
        .output();
    let exists = check.ok().map_or(false, |o| o.status.success());
    if !exists {
        let _ = Command::new("nft")
            .args(["add", "table", "ip", "travel-net-block"])
            .output();
        let _ = Command::new("nft")
            .args(["add", "set", "ip", "travel-net-block", "blocked", "{", "type", "ipv4_addr", ";", "}"])
            .output();
        let _ = Command::new("nft")
            .args(["add", "chain", "ip", "travel-net-block", "forward", "{", "type", "filter", "hook", "forward", "priority", "-1", ";", "policy", "accept", ";", "}"])
            .output();
        let _ = Command::new("nft")
            .args(["add", "rule", "ip", "travel-net-block", "forward", "ip", "saddr", "@blocked", "drop"])
            .output();
    }
}

pub fn block_client(ip: &str) -> Result<(), String> {
    ensure_block_table();
    let output = Command::new("nft")
        .args(["add", "element", "ip", "travel-net-block", "blocked", "{", ip, "}"])
        .output()
        .map_err(|e| format!("nft error: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

pub fn unblock_client(ip: &str) -> Result<(), String> {
    let output = Command::new("nft")
        .args(["delete", "element", "ip", "travel-net-block", "blocked", "{", ip, "}"])
        .output()
        .map_err(|e| format!("nft error: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

pub fn get_clients() -> Vec<DhcpClient> {
    let leases = read_leases();
    let active_ips = get_active_ips();
    let blocked_ips = get_blocked_ips();

    leases
        .into_iter()
        .map(|(expiry, mac, ip, hostname)| {
            let blocked = blocked_ips.contains(&ip);
            let active = active_ips.contains(&ip);
            DhcpClient { expiry, mac, ip, hostname, blocked, active }
        })
        .collect()
}

pub fn client_count() -> usize {
    get_clients().iter().filter(|c| !c.blocked).count()
}
