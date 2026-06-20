use crate::config::Config;
use std::fs;
use std::process::Command;

const DNSMASQ_CONF: &str = "/etc/travel-net/dnsmasq.conf";
const DNSMASQ_PID: &str = "/run/travel-net/dnsmasq.pid";

pub async fn start_dnsmasq(cfg: &Config) -> Result<(), String> {
    generate_conf(cfg)?;
    stop_dnsmasq().await;

    fs::create_dir_all("/run/travel-net").ok();

    let mut child = Command::new("dnsmasq")
        .args(["-C", DNSMASQ_CONF, "-x", DNSMASQ_PID])
        .spawn()
        .map_err(|e| format!("Failed to start dnsmasq: {e}"))?;

    let status = child.wait().map_err(|e| format!("dnsmasq wait: {e}"))?;
    if status.success() {
        tracing::info!("dnsmasq started (DHCP pool: {}-{})", cfg.dhcp_start, cfg.dhcp_end);
        Ok(())
    } else {
        Err(format!("dnsmasq exited with {status:?}"))
    }
}

pub async fn stop_dnsmasq() {
    if let Ok(pid) = fs::read_to_string(DNSMASQ_PID) {
        if let Ok(pid) = pid.trim().parse::<i32>() {
            let _ = Command::new("kill").args([&pid.to_string()]).output();
        }
    }
    let _ = Command::new("pkill").args(["-x", "dnsmasq"]).output();
}

pub fn is_running() -> bool {
    Command::new("pgrep").args(["-x", "dnsmasq"]).output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn generate_conf(cfg: &Config) -> Result<(), String> {
    let lease_secs = (cfg.dhcp_lease_hours * 3600).to_string();
    let conf = format!(
        r#"interface={iface}
bind-interfaces
server=127.0.0.53
domain-needed
bogus-priv
dhcp-range={dhcp_start},{dhcp_end},{lease_secs}s
dhcp-option=3,{ap_ip}
dhcp-option=6,{ap_ip}
dhcp-authoritative
dhcp-leasefile=/var/lib/misc/dnsmasq.leases
"#,
        iface = cfg.ap_interface,
        dhcp_start = cfg.dhcp_start,
        dhcp_end = cfg.dhcp_end,
        lease_secs = lease_secs,
        ap_ip = cfg.ap_ip,
    );

    fs::write(DNSMASQ_CONF, &conf).map_err(|e| format!("Write dnsmasq.conf: {e}"))
}
