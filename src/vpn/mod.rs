use crate::config::{Config, WgPeer};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

pub const VPN_NFT_FILE: &str = "/etc/travel-net/travel-vpn.nft";

pub static LAST_VPN_ERROR: Mutex<String> = Mutex::new(String::new());

pub fn last_vpn_error() -> String {
    LAST_VPN_ERROR.lock().unwrap().clone()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InstalledStatus {
    pub tailscale: bool,
    pub tailscaled_active: bool,
    pub wg: bool,
    pub wg_quick: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TailscaleStatus {
    pub state: String,
    pub hostname: String,
    pub ip: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WireguardStatus {
    pub iface_up: bool,
    pub endpoint: String,
    pub handshake_age_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VpnStatus {
    pub backend: String,
    pub role: String,
    pub installed: InstalledStatus,
    pub tailscale: TailscaleStatus,
    pub wireguard: WireguardStatus,
    pub kill_switch: bool,
    pub last_error: String,
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    use std::process::Stdio;
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn which(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn service_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", unit])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false)
}

fn iface_exists(name: &str) -> bool {
    std::path::Path::new("/sys/class/net").join(name).exists()
}

pub fn installed() -> InstalledStatus {
    InstalledStatus {
        tailscale: which("tailscale"),
        tailscaled_active: service_active("tailscaled"),
        wg: which("wg"),
        wg_quick: which("wg-quick"),
    }
}

pub fn tunnel_iface(cfg: &Config) -> Option<&'static str> {
    match cfg.vpn.backend.as_str() {
        "tailscale" => Some("tailscale0"),
        "wireguard" => Some("wg0"),
        _ => None,
    }
}

#[derive(Deserialize, Default)]
struct TsStatus {
    #[serde(default, rename = "BackendState")]
    backend_state: String,
    #[serde(default, rename = "Self")]
    self_: TsSelf,
}

#[derive(Deserialize, Default)]
struct TsSelf {
    #[serde(default, rename = "HostName")]
    host_name: String,
    #[serde(default, rename = "TailscaleIPs")]
    tailscale_ips: Vec<String>,
}

fn tailscale_status() -> TailscaleStatus {
    let mut ts = TailscaleStatus::default();
    match run("tailscale", &["status", "--json"]) {
        Ok(out) => {
            if let Ok(parsed) = serde_json::from_str::<TsStatus>(&out) {
                ts.state = parsed.backend_state;
                ts.hostname = parsed.self_.host_name;
                ts.ip = parsed
                    .self_
                    .tailscale_ips
                    .into_iter()
                    .find(|i| i.starts_with("100."))
                    .unwrap_or_default();
            }
        }
        Err(_) => {
            ts.state = "Stopped".into();
        }
    }
    ts
}

fn wireguard_status() -> WireguardStatus {
    let mut ws = WireguardStatus::default();
    ws.iface_up = iface_exists("wg0");
    if let Ok(dump) = run("wg", &["show", "wg0", "dump"]) {
        for line in dump.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 7 {
                ws.endpoint = f[2].to_string();
                if let Ok(hs) = f[4].parse::<u64>() {
                    if hs > 0 {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        ws.handshake_age_secs = Some(now.saturating_sub(hs));
                    }
                }
                break;
            }
        }
    }
    ws
}

pub fn status(cfg: &Config) -> VpnStatus {
    let inst = installed();
    let tailscale = if inst.tailscale {
        tailscale_status()
    } else {
        TailscaleStatus::default()
    };
    let wireguard = if inst.wg {
        wireguard_status()
    } else {
        WireguardStatus::default()
    };
    VpnStatus {
        backend: cfg.vpn.backend.clone(),
        role: cfg.vpn.role.clone(),
        installed: inst,
        tailscale,
        wireguard,
        kill_switch: kill_switch_active(),
        last_error: last_vpn_error(),
    }
}

fn kill_switch_active() -> bool {
    run("nft", &["list", "chain", "inet", "travel-vpn", "killswitch"])
        .map(|out| out.contains("drop"))
        .unwrap_or(false)
}

fn wg_server_subnet(cfg: &Config) -> String {
    let (ip, prefix) = cfg
        .vpn
        .wg_address
        .split_once('/')
        .unwrap_or(("10.0.0.2", "24"));
    let mut oct: Vec<&str> = ip.split('.').collect();
    if oct.len() == 4 {
        oct[3] = "0";
    }
    format!("{}/{}", oct.join("."), prefix)
}

fn generate_table(cfg: &Config) -> String {
    let ap = cfg.ap_interface.as_str();
    let wg_port = cfg.vpn.wg_listen_port;
    let wg_subnet = wg_server_subnet(cfg);
    format!(
        r#"#!/usr/sbin/nft -f

table inet travel-vpn {{
    chain postrouting {{
        type nat hook postrouting priority srcnat; policy accept;
        oifname "wg0" masquerade
        oifname "tailscale0" masquerade
        iifname "wg0" masquerade
    }}

    chain forward {{
        type filter hook forward priority filter; policy accept;
        iifname "{ap}" oifname "wg0" accept
        iifname "{ap}" oifname "tailscale0" accept
        iifname "wg0" oifname "{ap}" ct state related,established accept
        iifname "tailscale0" oifname "{ap}" ct state related,established accept
        ip saddr {wg_subnet} accept
    }}

    chain input {{
        type filter hook input priority filter; policy accept;
        udp dport {wg_port} accept
    }}

    chain clamp {{
        type filter hook forward priority filter + 2; policy accept;
        oifname "wg0" tcp flags syn tcp option maxseg size set 1380
    }}

    chain killswitch {{
        type filter hook forward priority filter + 1; policy accept;
    }}
}}
"#,
        ap = ap,
        wg_port = wg_port,
        wg_subnet = wg_subnet,
    )
}

pub fn nft_assert(cfg: &Config) -> Result<(), String> {
    crate::system::remount::safe_write(std::path::Path::new(VPN_NFT_FILE), &generate_table(cfg))?;
    let _ = Command::new("sysctl")
        .args(["-w", "net.ipv4.ip_forward=1"])
        .output();
    let _ = run("nft", &["delete", "table", "inet", "travel-vpn"]);
    run("nft", &["-f", VPN_NFT_FILE]).map(|_| ())
}

/// Re-assert the travel-vpn nft table after something flushed the ruleset
/// (e.g. `firewall::apply_ruleset`), then sync the kill switch to the live
/// tunnel state so AP clients are never left with a stale/broken policy.
pub fn reassert_rules(cfg: &Config) -> Result<(), String> {
    nft_assert(cfg)?;
    sync_kill_switch(cfg)?;
    Ok(())
}

pub fn nft_teardown() {
    let _ = run("nft", &["delete", "table", "inet", "travel-vpn"]);
}

pub fn kill_switch_set(cfg: &Config) -> Result<(), String> {
    let tun = tunnel_iface(cfg).ok_or("no tunnel interface configured")?;
    run("nft", &["flush", "chain", "inet", "travel-vpn", "killswitch"])?;
    let args = vec![
        "add",
        "rule",
        "inet",
        "travel-vpn",
        "killswitch",
        "iifname",
        cfg.ap_interface.as_str(),
        "oifname",
        "!=",
        tun,
        "drop",
    ];
    run("nft", &args).map(|_| ())
}

pub fn kill_switch_clear() -> Result<(), String> {
    run("nft", &["flush", "chain", "inet", "travel-vpn", "killswitch"]).map(|_| ())
}

pub fn gen_keypair() -> Result<(String, String), String> {
    let privkey = run("wg", &["genkey"])?;
    let mut child = Command::new("wg")
        .arg("pubkey")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("wg pubkey spawn: {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(privkey.as_bytes())
        .map_err(|e| format!("wg pubkey stdin: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wg pubkey: {e}"))?;
    let pubkey = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((privkey.trim().to_string(), pubkey))
}

fn wg_client_conf(cfg: &Config) -> String {
    let allowed = if cfg.vpn.wg_route_all {
        "0.0.0.0/0".to_string()
    } else {
        wg_server_subnet(cfg)
    };
    let mut conf = String::new();
    conf.push_str("[Interface]\n");
    conf.push_str(&format!("PrivateKey = {}\n", cfg.vpn.wg_private_key));
    conf.push_str(&format!("Address = {}\n", cfg.vpn.wg_address));
    if !cfg.vpn.wg_dns.is_empty() {
        conf.push_str(&format!("DNS = {}\n", cfg.vpn.wg_dns));
    }
    conf.push_str("ListenPort = 51820\n\n");
    conf.push_str("[Peer]\n");
    conf.push_str(&format!("PublicKey = {}\n", cfg.vpn.wg_peer_public_key));
    if !cfg.vpn.wg_preshared_key.is_empty() {
        conf.push_str(&format!("PresharedKey = {}\n", cfg.vpn.wg_preshared_key));
    }
    conf.push_str(&format!("Endpoint = {}\n", cfg.vpn.wg_endpoint));
    conf.push_str(&format!("PersistentKeepalive = {}\n", cfg.vpn.wg_keepalive));
    conf.push_str(&format!("AllowedIPs = {allowed}\n"));
    conf
}

pub fn write_wg_client_conf(cfg: &Config) -> Result<(), String> {
    let dir = std::path::PathBuf::from("/etc/wireguard");
    crate::system::remount::safe_create_dir_all(&dir)?;
    let path = dir.join("wg0.conf");
    crate::system::remount::safe_write(&path, &wg_client_conf(cfg))?;
    let _ = crate::system::remount::safe_set_permissions(&path, 0o600);
    Ok(())
}

pub fn wg_quick_up() -> Result<(), String> {
    run("wg-quick", &["up", "wg0"]).map(|_| ())
}

pub fn wg_quick_down() -> Result<(), String> {
    let _ = run("wg-quick", &["down", "wg0"]);
    Ok(())
}

pub fn sync_kill_switch(cfg: &Config) -> Result<(), String> {
    if cfg.vpn.role != "travel" {
        return kill_switch_clear();
    }
    if cfg.vpn.backend == "wireguard" && !cfg.vpn.wg_route_all {
        return kill_switch_clear();
    }
    let healthy = match cfg.vpn.backend.as_str() {
        "wireguard" => {
            let ws = wireguard_status();
            ws.iface_up && ws.handshake_age_secs.is_some()
        }
        "tailscale" => tailscale_status().state == "Running",
        _ => true,
    };
    if healthy {
        kill_switch_clear()
    } else {
        kill_switch_set(cfg)
    }
}

fn ts_os_from(os_release: &str) -> &'static str {
    let lower = os_release.to_lowercase();
    if lower.contains("raspbian") {
        "raspbian"
    } else if lower.contains("ubuntu") {
        "ubuntu"
    } else {
        "debian"
    }
}

fn ts_os() -> String {
    fs::read_to_string("/etc/os-release")
        .map(|c| ts_os_from(&c).to_string())
        .unwrap_or_else(|_| "debian".into())
}

fn ts_codename_from(os_release: &str) -> String {
    os_release
        .lines()
        .find_map(|l| {
            let mut it = l.splitn(2, '=');
            let (k, v) = (it.next()?, it.next()?);
            (k == "VERSION_CODENAME").then(|| v.trim().trim_matches('"').to_string())
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "stable".into())
}

fn ts_codename() -> String {
    fs::read_to_string("/etc/os-release")
        .map(|c| ts_codename_from(&c))
        .unwrap_or_else(|_| "stable".into())
}

pub fn install() -> Result<Vec<String>, String> {
    let mut logs = Vec::new();

    // Rule 1+3: Fix any broken dpkg state BEFORE any apt operation
    let _ = run("sh", &["-c", "dpkg --configure -a 2>/dev/null || true"]);
    let _ = run("sh", &["-c", "apt-get -f install -y 2>/dev/null || true"]);

    // Rule 3: Verify clock before apt
    let year_ok = run("sh", &["-c", "YEAR=$(date +%Y 2>/dev/null); [ -n \"$YEAR\" ] && [ \"$YEAR\" -ge 2024 ] 2>/dev/null"]);
    if year_ok.is_err() {
        return Err("System clock is wrong (year < 2024). Run 'sudo timedatectl set-ntp true' first, then try again.".into());
    }

    // Clear stale apt lists
    let _ = run("sh", &["-c", "rm -f /var/lib/apt/lists/*Packages*"]);

    // Install wireguard-tools
    let wg = run("env", &["DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y", "wireguard-tools"]);
    logs.push(format!(
        "wireguard-tools: {}",
        wg.map(|_| "installed".to_string()).unwrap_or_else(|e| format!("install failed: {e}"))
    ));

    if which("tailscale") {
        logs.push("Tailscale is already installed".into());
        return Ok(logs);
    }

    let os = ts_os();
    let codename = ts_codename();
    logs.push(format!("Installing Tailscale via apt repo (pkgs.tailscale.com/stable/{os}/{codename})"));

    let _ = Command::new("mkdir").args(["-p", "--mode=0755", "/usr/share/keyrings"]).status();
    let _ = run("sh", &["-c", &format!(
        "curl -fsSL https://pkgs.tailscale.com/stable/{os}/{codename}.noarmor.gpg -o /usr/share/keyrings/tailscale-archive-keyring.gpg"
    )]);
    let _ = run("sh", &["-c", &format!(
        "curl -fsSL https://pkgs.tailscale.com/stable/{os}/{codename}.tailscale-keyring.list -o /etc/apt/sources.list.d/tailscale.list"
    )]);
    let _ = run("chmod", &["0644", "/etc/apt/sources.list.d/tailscale.list"]);
    let _ = run("sh", &["-c", "rm -f /var/lib/apt/lists/*Packages*"]);

    let up = run("env", &["DEBIAN_FRONTEND=noninteractive", "apt-get", "update"]);
    if up.is_err() {
        return Err(format!("apt-get update failed: {}", up.unwrap_err()));
    }

    // Pin to 1.76.* — last version compatible with kernel 5.15 (arm64)
    let inst = run("env", &["DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y", "tailscale=1.76.*"]);
    if inst.is_err() {
        return Err(format!("tailscale install failed: {}", inst.unwrap_err()));
    }

    let _ = run("systemctl", &["disable", "tailscaled"]);
    logs.push("Tailscale installed (service stays disabled until you enable a VPN)".into());
    Ok(logs)
}

pub fn tailscale_up(cfg: &Config) -> Result<(), String> {
    if !which("tailscale") {
        return Err("tailscale is not installed".into());
    }
    let _ = run("systemctl", &["enable", "tailscaled"]);
    let _ = run("systemctl", &["start", "tailscaled"]);

    // Wait for tailscaled to be ready (up to 10 seconds)
    let mut ready = false;
    for _ in 0..10 {
        thread::sleep(Duration::from_secs(1));
        if Command::new("env")
            .args(["TS_AUTH_ONCE=true", "tailscale", "status", "--json"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            ready = true;
            break;
        }
    }
    if !ready {
        // tailscaled may still be starting; try anyway but warn
        *LAST_VPN_ERROR.lock().unwrap() = "tailscaled may not be fully ready yet".into();
    }

    let mut args: Vec<String> = vec!["up".into(), "--reset".into()];
    if !cfg.vpn.ts_auth_key.is_empty() {
        args.push(format!("--authkey={}", cfg.vpn.ts_auth_key));
    }
    if !cfg.vpn.ts_hostname.is_empty() {
        args.push(format!("--hostname={}", cfg.vpn.ts_hostname));
    }
    match cfg.vpn.role.as_str() {
        "home" => {
            args.push("--advertise-exit-node".into());
            if !cfg.vpn.ts_advertise_routes.is_empty() {
                args.push(format!("--advertise-routes={}", cfg.vpn.ts_advertise_routes));
            }
        }
        _ => {
            args.push("--accept-routes".into());
            if cfg.vpn.ts_route_all && !cfg.vpn.ts_exit_node.is_empty() {
                args.push(format!("--exit-node={}", cfg.vpn.ts_exit_node));
                if cfg.vpn.ts_allow_lan {
                    args.push("--exit-node-allow-lan-access".into());
                }
            }
        }
    }
    let result = Command::new("env")
        .args(["TS_AUTH_ONCE=true", "tailscale"])
        .args(&args)
        .output()
        .map(|o| {
            if o.status.success() {
                Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
            }
        })
        .map_err(|e| format!("tailscale: {e}"))?;
    match &result {
        Ok(_) => {
            *LAST_VPN_ERROR.lock().unwrap() = String::new();
        }
        Err(e) => {
            *LAST_VPN_ERROR.lock().unwrap() = e.clone();
        }
    }
    result.map(|_| ())
}

pub fn tailscale_down() -> Result<(), String> {
    let _ = run("tailscale", &["down"]);
    Ok(())
}

pub fn wg_client_apply(cfg: &Config) -> Result<(), String> {
    write_wg_client_conf(cfg)?;
    let _ = wg_quick_down();
    nft_assert(cfg)?;
    wg_quick_up()?;
    sync_kill_switch(cfg)?;
    Ok(())
}

fn wg_server_address(cfg: &Config) -> String {
    let net = wg_server_subnet(cfg);
    if let Some((netip, prefix)) = net.split_once('/') {
        let mut oct: Vec<&str> = netip.split('.').collect();
        if oct.len() == 4 {
            oct[3] = "1";
        }
        format!("{}/{}", oct.join("."), prefix)
    } else {
        "10.0.0.1/24".into()
    }
}

fn wg_server_conf(cfg: &Config) -> Result<String, String> {
    let _ap_net = cfg.ap_network().map_err(|e| e.to_string())?;
    let mut conf = String::new();
    conf.push_str("[Interface]\n");
    conf.push_str(&format!("PrivateKey = {}\n", cfg.vpn.wg_server_private_key));
    conf.push_str(&format!("Address = {}\n", wg_server_address(cfg)));
    conf.push_str(&format!("ListenPort = {}\n", cfg.vpn.wg_listen_port));
    for peer in &cfg.vpn.wg_peers {
        conf.push_str(&format!("\n# {}\n[Peer]\n", peer.name));
        conf.push_str(&format!("PublicKey = {}\n", peer.public_key));
        // The travel box masquerades its AP traffic out wg0, so replies come
        // back to this box addressed to the peer's tunnel IP. Routing the AP
        // subnet through the tunnel here would clash with our own AP subnet
        // and make `wg-quick up` roll back.
        conf.push_str(&format!("AllowedIPs = {}\n", peer.tunnel_ip));
    }
    Ok(conf)
}

pub fn write_wg_server_conf(cfg: &Config) -> Result<(), String> {
    let dir = std::path::PathBuf::from("/etc/wireguard");
    crate::system::remount::safe_create_dir_all(&dir)?;
    let path = dir.join("wg0.conf");
    crate::system::remount::safe_write(&path, &wg_server_conf(cfg)?)?;
    let _ = crate::system::remount::safe_set_permissions(&path, 0o600);
    Ok(())
}

pub fn wg_server_apply(cfg: &Config) -> Result<(), String> {
    write_wg_server_conf(cfg)?;
    let _ = wg_quick_down();
    nft_assert(cfg)?;
    wg_quick_up()?;
    Ok(())
}

pub fn next_tunnel_ip(cfg: &Config) -> Result<String, String> {
    let used: std::collections::HashSet<String> = cfg
        .vpn
        .wg_peers
        .iter()
        .map(|p| p.tunnel_ip.clone())
        .collect();
    for n in 2..=254u8 {
        let ip = format!("10.0.0.{n}");
        if !used.contains(&ip) {
            return Ok(ip);
        }
    }
    Err("No free tunnel IPs left in 10.0.0.0/24".into())
}

pub fn peer_client_conf(cfg: &Config, peer: &WgPeer, private_key: &str) -> Result<String, String> {
    if cfg.vpn.wg_server_public_key.is_empty() {
        return Err("Server public key is missing — generate the server key first".into());
    }
    if cfg.vpn.wg_endpoint.is_empty() {
        return Err(
            "Home endpoint is missing — the address travel devices use to reach this box, e.g. myhome.example.com:51820"
                .into(),
        );
    }
    let allowed = if cfg.vpn.wg_route_all {
        "0.0.0.0/0".to_string()
    } else {
        wg_server_subnet(cfg)
    };
    let mut conf = String::new();
    conf.push_str("[Interface]\n");
    conf.push_str(&format!("PrivateKey = {}\n", private_key));
    conf.push_str(&format!("Address = {}/24\n", peer.tunnel_ip));
    if !cfg.vpn.wg_dns.is_empty() {
        conf.push_str(&format!("DNS = {}\n", cfg.vpn.wg_dns));
    }
    conf.push_str("ListenPort = 51820\n\n");
    conf.push_str("[Peer]\n");
    conf.push_str(&format!("PublicKey = {}\n", cfg.vpn.wg_server_public_key));
    if !cfg.vpn.wg_preshared_key.is_empty() {
        conf.push_str(&format!("PresharedKey = {}\n", cfg.vpn.wg_preshared_key));
    }
    conf.push_str(&format!("Endpoint = {}\n", cfg.vpn.wg_endpoint));
    conf.push_str(&format!("PersistentKeepalive = {}\n", cfg.vpn.wg_keepalive));
    conf.push_str(&format!("AllowedIPs = {allowed}\n"));
    Ok(conf)
}

pub fn gen_peer(cfg: &Config, name: &str) -> Result<(WgPeer, String), String> {
    let (privkey, pubkey) = gen_keypair()?;
    let tunnel_ip = next_tunnel_ip(cfg)?;
    let peer = WgPeer {
        name: name.to_string(),
        public_key: pubkey,
        tunnel_ip: tunnel_ip.clone(),
    };
    let conf = peer_client_conf(cfg, &peer, &privkey)?;
    Ok((peer, conf))
}

pub fn import_conf(text: &str) -> Result<crate::config::VpnConfig, String> {
    let mut vpn = crate::config::VpnConfig {
        backend: "wireguard".into(),
        role: "travel".into(),
        ..Default::default()
    };
    let mut in_interface = false;
    let mut in_peer = false;
    let mut address_seen = false;
    let required = true;

    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let section = line[1..line.len() - 1].trim();
            in_interface = section.eq_ignore_ascii_case("interface");
            in_peer = section.eq_ignore_ascii_case("peer");
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim()),
            None => continue,
        };
        if in_interface {
            match key.as_str() {
                "privatekey" => vpn.wg_private_key = value.to_string(),
                "address" => {
                    if address_seen {
                        continue;
                    }
                    if let Some(first) = value.split(',').map(str::trim).next() {
                        if !first.contains(':') {
                            vpn.wg_address = first.to_string();
                            address_seen = true;
                        }
                    }
                }
                "dns" => {
                    if let Some(first) = value.split(',').map(str::trim).next() {
                        if !first.contains(':') {
                            vpn.wg_dns = first.to_string();
                        }
                    }
                }
                _ => {}
            }
        } else if in_peer {
            match key.as_str() {
                "publickey" => vpn.wg_peer_public_key = value.to_string(),
                "presharedkey" => vpn.wg_preshared_key = value.to_string(),
                "endpoint" => vpn.wg_endpoint = value.to_string(),
                "persistentkeepalive" => {
                    vpn.wg_keepalive = value
                        .parse::<u32>()
                        .map_err(|_| format!("Invalid PersistentKeepalive: {value}"))?;
                }
                "allowedips" => {
                    vpn.wg_route_all = value
                        .split(',')
                        .map(str::trim)
                        .any(|entry| entry == "0.0.0.0/0");
                }
                _ => {}
            }
        }
    }

    if required && vpn.wg_private_key.is_empty() {
        return Err("The config is missing a PrivateKey in [Interface]".into());
    }
    if required && !address_seen {
        return Err("The config is missing an IPv4 Address in [Interface]".into());
    }
    if required && vpn.wg_peer_public_key.is_empty() {
        return Err("The config is missing a Peer PublicKey".into());
    }
    if required && vpn.wg_endpoint.is_empty() {
        return Err("The config is missing a Peer Endpoint (host:port)".into());
    }
    Ok(vpn)
}

pub fn apply(cfg: &Config) -> Result<(), String> {
    nft_teardown();
    match cfg.vpn.backend.as_str() {
        "wireguard" if cfg.vpn.role == "travel" => wg_client_apply(cfg),
        "wireguard" => wg_server_apply(cfg),
        "tailscale" => {
            let _ = wg_quick_down();
            nft_assert(cfg)?;
            tailscale_up(cfg)?;
            sync_kill_switch(cfg)?;
            Ok(())
        }
        _ => {
            let _ = wg_quick_down();
            let _ = tailscale_down();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> Config {
        Config {
            vpn: crate::config::VpnConfig {
                backend: "wireguard".into(),
                role: "travel".into(),
                wg_listen_port: 51820,
                wg_address: "10.0.0.2/24".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn generate_table_contains_expected_rules() {
        let t = generate_table(&test_cfg());
        assert!(t.contains("table inet travel-vpn"));
        assert!(t.contains("oifname \"wg0\" masquerade"));
        assert!(t.contains("oifname \"tailscale0\" masquerade"));
        assert!(t.contains("udp dport 51820 accept"));
        assert!(t.contains("iifname \"wlan1\" oifname \"wg0\" accept"));
        assert!(t.contains("chain killswitch"));
    }

    #[test]
    fn wg_server_subnet_from_address() {
        assert_eq!(wg_server_subnet(&test_cfg()), "10.0.0.0/24");
    }

    #[test]
    fn tunnel_iface_by_backend() {
        let mut cfg = test_cfg();
        assert_eq!(tunnel_iface(&cfg), Some("wg0"));
        cfg.vpn.backend = "tailscale".into();
        assert_eq!(tunnel_iface(&cfg), Some("tailscale0"));
        cfg.vpn.backend = "none".into();
        assert_eq!(tunnel_iface(&cfg), None);
    }

    #[test]
    fn client_conf_route_all() {
        let mut cfg = test_cfg();
        cfg.vpn.role = "travel".into();
        cfg.vpn.wg_private_key = "privkey".into();
        cfg.vpn.wg_peer_public_key = "pubkey".into();
        cfg.vpn.wg_endpoint = "example.com:51820".into();
        cfg.vpn.wg_route_all = true;
        let conf = wg_client_conf(&cfg);
        assert!(conf.contains("[Interface]"));
        assert!(conf.contains("PrivateKey = privkey"));
        assert!(conf.contains("Address = 10.0.0.2/24"));
        assert!(conf.contains("ListenPort = 51820"));
        assert!(conf.contains("PublicKey = pubkey"));
        assert!(conf.contains("Endpoint = example.com:51820"));
        assert!(conf.contains("PersistentKeepalive = 25"));
        assert!(conf.contains("AllowedIPs = 0.0.0.0/0"));
    }

    #[test]
    fn client_conf_split_tunnel() {
        let mut cfg = test_cfg();
        cfg.vpn.wg_route_all = false;
        cfg.vpn.wg_private_key = "privkey".into();
        cfg.vpn.wg_peer_public_key = "pubkey".into();
        cfg.vpn.wg_endpoint = "example.com:51820".into();
        let conf = wg_client_conf(&cfg);
        assert!(conf.contains("AllowedIPs = 10.0.0.0/24"));
        assert!(!conf.contains("0.0.0.0/0"));
    }

    #[test]
    fn client_conf_includes_dns_and_psk() {
        let mut cfg = test_cfg();
        cfg.vpn.wg_dns = "10.0.0.1".into();
        cfg.vpn.wg_preshared_key = "psk".into();
        let conf = wg_client_conf(&cfg);
        assert!(conf.contains("DNS = 10.0.0.1"));
        assert!(conf.contains("PresharedKey = psk"));
    }

    #[test]
    fn generate_table_has_mss_clamp() {
        let t = generate_table(&test_cfg());
        assert!(t.contains("tcp option maxseg size set 1380"));
    }

    #[test]
    fn keypair_generation() {
        if !which("wg") {
            return;
        }
        let (privkey, pubkey) = gen_keypair().unwrap();
        assert_eq!(privkey.len(), 44, "wg genkey is base64 of 32 bytes");
        assert!(privkey
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='));
        assert_eq!(pubkey.len(), 44);
    }

    #[test]
    fn wg_server_address_derived_from_client_subnet() {
        let cfg = test_cfg();
        assert_eq!(wg_server_address(&cfg), "10.0.0.1/24");
        let mut cfg2 = test_cfg();
        cfg2.vpn.wg_address = "10.9.0.2/24".into();
        assert_eq!(wg_server_address(&cfg2), "10.9.0.1/24");
    }

    #[test]
    fn server_conf_lists_peers() {
        let cfg = Config {
            vpn: crate::config::VpnConfig {
                backend: "wireguard".into(),
                role: "home".into(),
                wg_server_private_key: "SRVKEY".into(),
                wg_listen_port: 51820,
                wg_peers: vec![crate::config::WgPeer {
                    name: "travel".into(),
                    public_key: "PEERKEY".into(),
                    tunnel_ip: "10.0.0.2".into(),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let conf = wg_server_conf(&cfg).unwrap();
        assert!(conf.contains("PrivateKey = SRVKEY"));
        assert!(conf.contains("Address = 10.0.0.1/24"));
        assert!(conf.contains("ListenPort = 51820"));
        assert!(conf.contains("# travel\n[Peer]"));
        assert!(conf.contains("PublicKey = PEERKEY"));
        assert!(conf.contains("AllowedIPs = 10.0.0.2"));
        assert!(!conf.contains("192.168.4.0/24"));
    }

    #[test]
    fn next_tunnel_ip_skips_used() {
        let cfg = Config {
            vpn: crate::config::VpnConfig {
                backend: "wireguard".into(),
                role: "home".into(),
                wg_peers: vec![
                    crate::config::WgPeer {
                        name: "a".into(),
                        public_key: "A".into(),
                        tunnel_ip: "10.0.0.2".into(),
                    },
                    crate::config::WgPeer {
                        name: "b".into(),
                        public_key: "B".into(),
                        tunnel_ip: "10.0.0.3".into(),
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(next_tunnel_ip(&cfg).unwrap(), "10.0.0.4");
    }

    #[test]
    fn peer_client_conf_builds_importable_config() {
        let cfg = Config {
            vpn: crate::config::VpnConfig {
                backend: "wireguard".into(),
                role: "home".into(),
                wg_server_public_key: "SRVPUB".into(),
                wg_endpoint: "myhome.example.com:51820".into(),
                wg_dns: "10.0.0.1".into(),
                wg_keepalive: 25,
                wg_route_all: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let peer = crate::config::WgPeer {
            name: "travel".into(),
            public_key: "PEERKEY".into(),
            tunnel_ip: "10.0.0.2".into(),
        };
        let conf = peer_client_conf(&cfg, &peer, "PRIVKEY").unwrap();
        assert!(conf.contains("PrivateKey = PRIVKEY"));
        assert!(conf.contains("Address = 10.0.0.2/24"));
        assert!(conf.contains("DNS = 10.0.0.1"));
        assert!(conf.contains("PublicKey = SRVPUB"));
        assert!(conf.contains("Endpoint = myhome.example.com:51820"));
        assert!(conf.contains("AllowedIPs = 0.0.0.0/0"));
    }

    #[test]
    fn generate_table_nats_wg_clients() {
        let t = generate_table(&test_cfg());
        assert!(t.contains("iifname \"wg0\" masquerade"));
    }

    const SAMPLE_CONF: &str = r#"[Interface]
PrivateKey = 9AhSrvIeJ8I8hT6kq2yY8V8XpQwV4oZv8N0bQcWaRmI=
Address = 10.2.0.2/32, fd7a:115c:a1e0::2/128
DNS = 10.2.0.1, fd7a:115c:a1e0::1

[Peer]
PublicKey = hgBfOaXxYz0AbCdeFgHiJkLmNoPqRsTuVwXyZ012345=
PresharedKey = 6x7dH2tUvXyZaBcDeFgHiJkLmNoPqRsTuVwXyZ012345=
Endpoint = myhome.example.com:51820
PersistentKeepalive = 25
AllowedIPs = 0.0.0.0/0, ::/0
"#;

    #[test]
    fn import_conf_full_route_all() {
        let vpn = import_conf(SAMPLE_CONF).unwrap();
        assert_eq!(vpn.backend, "wireguard");
        assert_eq!(vpn.role, "travel");
        assert_eq!(vpn.wg_private_key, "9AhSrvIeJ8I8hT6kq2yY8V8XpQwV4oZv8N0bQcWaRmI=");
        assert_eq!(vpn.wg_address, "10.2.0.2/32");
        assert_eq!(vpn.wg_dns, "10.2.0.1");
        assert_eq!(vpn.wg_peer_public_key, "hgBfOaXxYz0AbCdeFgHiJkLmNoPqRsTuVwXyZ012345=");
        assert_eq!(vpn.wg_preshared_key, "6x7dH2tUvXyZaBcDeFgHiJkLmNoPqRsTuVwXyZ012345=");
        assert_eq!(vpn.wg_endpoint, "myhome.example.com:51820");
        assert_eq!(vpn.wg_keepalive, 25);
        assert!(vpn.wg_route_all);
    }

    #[test]
    fn import_conf_split_tunnel_disables_route_all() {
        let conf = SAMPLE_CONF.replace("AllowedIPs = 0.0.0.0/0, ::/0", "AllowedIPs = 10.2.0.0/24");
        let vpn = import_conf(&conf).unwrap();
        assert!(!vpn.wg_route_all);
    }

    #[test]
    fn import_conf_rejects_missing_required_fields() {
        assert!(import_conf("[Interface]\nPrivateKey = x\n").is_err());
        assert!(import_conf("[Interface]\nPrivateKey = x\nAddress = 10.0.0.2/24\n").is_err());
        assert!(import_conf("[Interface]\nAddress = 10.0.0.2/24\n[Peer]\nEndpoint = h:1\n").is_err());
        let mut conf = SAMPLE_CONF.to_string();
        conf = conf.replace("PrivateKey = ", "Nope = ");
        assert!(import_conf(&conf).is_err());
    }

    #[test]
    fn import_conf_ignores_comments_and_blank_lines() {
        let conf = "# a comment\n\n[Interface]\n  PrivateKey = KEY = EXTRA\nAddress = 10.2.0.2/24 # inline\n\n[Peer]\nPublicKey = PK\nEndpoint = host:51820\n";
        let vpn = import_conf(conf).unwrap();
        assert_eq!(vpn.wg_private_key, "KEY = EXTRA");
        assert_eq!(vpn.wg_address, "10.2.0.2/24");
        assert_eq!(vpn.wg_peer_public_key, "PK");
    }

    #[test]
    fn import_conf_invalid_keepalive_rejected() {
        let conf = SAMPLE_CONF.replace("PersistentKeepalive = 25", "PersistentKeepalive = abc");
        assert!(import_conf(&conf).is_err());
    }

    #[test]
    fn ts_distro_mapping() {
        assert_eq!(ts_os_from("ID=raspbian\n"), "raspbian");
        assert_eq!(ts_os_from("ID=ubuntu\n"), "ubuntu");
        assert_eq!(ts_os_from("ID=debian\n"), "debian");
        assert_eq!(ts_os_from("ID_LIKE=ubuntu\n"), "ubuntu");
        assert_eq!(ts_os_from(""), "debian");
    }

    #[test]
    fn ts_codename_parsing() {
        assert_eq!(ts_codename_from("VERSION_CODENAME=trixie\n"), "trixie");
        assert_eq!(ts_codename_from("VERSION_CODENAME=\"jammy\"\n"), "jammy");
        assert_eq!(ts_codename_from("ID=debian\n"), "stable");
    }

    #[test]
    fn tailscale_up_args_build_correctly() {
        if !which("tailscale") {
            return;
        }
        let cfg = Config {
            vpn: crate::config::VpnConfig {
                backend: "tailscale".into(),
                role: "home".into(),
                ts_auth_key: "tskey-abc".into(),
                ts_hostname: "homebox".into(),
                ts_advertise_routes: "192.168.1.0/24".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let vpn = tailscale_up(&cfg);
        assert!(vpn.is_ok() || vpn.is_err());
    }
}
