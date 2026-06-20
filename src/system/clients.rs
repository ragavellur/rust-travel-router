use serde::Serialize;
use std::fs;

#[derive(Debug, Serialize)]
pub struct DhcpClient {
    pub expiry: String,
    pub mac: String,
    pub ip: String,
    pub hostname: String,
}

pub fn get_clients() -> Vec<DhcpClient> {
    let lease_paths = [
        "/var/lib/misc/dnsmasq.leases",
        "/var/lib/dnsmasq/dnsmasq.leases",
    ];

    let data = lease_paths
        .iter()
        .find_map(|p| fs::read_to_string(p).ok())
        .unwrap_or_default();

    data.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            Some(DhcpClient {
                expiry: parts[0].to_string(),
                mac: parts[1].to_uppercase(),
                ip: parts[2].to_string(),
                hostname: if parts[3] == "*" {
                    String::new()
                } else {
                    parts[3].to_string()
                },
            })
        })
        .collect()
}

pub fn client_count() -> usize {
    let lease_paths = [
        "/var/lib/misc/dnsmasq.leases",
        "/var/lib/dnsmasq/dnsmasq.leases",
    ];
    lease_paths
        .iter()
        .find_map(|p| {
            fs::read_to_string(p)
                .ok()
                .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        })
        .unwrap_or(0)
}
