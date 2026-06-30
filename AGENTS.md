# AGENTS — travel-net: Rust NAT Router

## Project

**Single-binary travel NAT router** — turn any Linux SBC with WiFi into a portable NAT router with a web UI (axum + tokio, embedded HTML/CSS). Inspired by ESP32 NAT Router.

**GitHub**: https://github.com/ragavellur/rust-travel-router
**APT repo**: https://ragavellur.github.io/rust-travel-router/ (gh-pages branch)
**Sources.list**: `deb [trusted=yes] https://ragavellur.github.io/rust-travel-router/ ./`

## Hardware Inventory

### NanoPi NEO Air
- **OS**: DietPi (Debian 13 Trixie), armhf
- **IP**: 192.168.200.114 (DHCP, may change)
- **User/Pass**: root / raga@098
- **WiFi**: brcmfmac BCM43430 (SDIO)
- **AP backend**: hostapd + dnsmasq + nftables (WpaSupplicant path)
- **STA connect**: wpa_supplicant
- **Channel constraint**: AP and STA must share same channel (brcmfmac limitation). Config uses channel 4.
- **mDNS**: `travel-router.local`, `nanopi-neo-air.local`

### Radxa Cubie A5E
- **OS**: Debian 11 Bullseye, arm64, kernel 5.15.147
- **IP**: 192.168.1.35
- **User/Pass**: radxa / radxa
- **WiFi**: AIC8800D80 (SDIO, single MAC: 94:ba:06:49:ae:38), driver `aic8800_fdrv`
- **AP backend**: NetworkManager hotspot (NM backend) — hostapd FAILS with "Failed to set beacon parameters"
- **STA connect**: NetworkManager (nmcli)
- **Ethernet**: 2 ports (eth0, eth1), both down, not used by travel-net
- **Interface creation**: `iw phy phy0 interface add wlan1 type managed` (NOT `type __ap`; type __ap creates interface but AP type doesn't make hostapd work)

### Radxa Cubie A7A
- **OS**: Debian 11 Bullseye, arm64
- **IP**: 192.168.200.189
- **User/Pass**: radxa / radxa
- **WiFi**: AIC8800D80 (USB, dual MAC), driver `aic8800_bsp`/`aic8800_usb`
- **AP backend**: NM hotspot (not yet ported from Python Flask)
- **STA connect**: NetworkManager
- **Status**: Not yet deployed with travel-net. Currently running Python Flask portal.

## Directory Structure

```
/Users/Shared/Radxa_Cubie_A7A/travel-net/
├── AGENTS.md                 ← this file
├── Cargo.toml                ← Rust project manifest
├── README.md                 ← full docs
├── .gitignore
├── config/
│   └── etc/travel-net/
│       └── config.json       ← default config
├── debian/
│   ├── control               ← .deb package metadata
│   ├── postinst              ← post-install script
│   ├── prerm                 ← pre-remove script
│   └── travel-net.service    ← systemd unit
├── nftables/
│   └── travel-net.nft        ← nftables ruleset
├── templates/                ← embedded HTML templates
│   ├── index.html            ← dashboard
│   ├── scan.html             ← WiFi scan page
│   ├── config.html           ← settings page
│   ├── setup.html            ← wizard
│   ├── logs.html             ← journal viewer
│   └── login.html            ← auth page
├── src/
│   ├── main.rs               ← entry point, orchestrates services
│   ├── config.rs             ← JSON config load/save
│   ├── templates.rs          ← HTML template loading
│   ├── ap/
│   │   ├── mod.rs            ← start_ap() backend dispatch
│   │   ├── interface.rs      ← iw interface create/delete
│   │   ├── hostapd.rs        ← hostapd control
│   │   ├── networkmanager.rs ← NM hotspot control (backend for AIC8800)
│   │   └── apply.rs          ← config apply at runtime
│   ├── dhcp/
│   │   └── mod.rs            ← dnsmasq control
│   ├── firewall/
│   │   └── mod.rs            ← nftables rules + IP forwarding
│   ├── wifi/
│   │   ├── mod.rs            ← Backend enum + detect_backend()
│   │   ├── connect.rs        ← Connect to WiFi (NM + wpa_supplicant)
│   │   ├── status.rs         ← Link status
│   │   └── scan.rs           ← WiFi scanning
│   ├── web/
│   │   ├── api.rs            ← REST API endpoints
│   │   ├── pages.rs          ← HTML page routes
│   │   └── auth.rs           ← Session-based auth
│   ├── system/
│   │   └── mod.rs            ← Reboot/shutdown/journald logs
│   └── ... (other modules)
└── target/                   ← build output (gitignored)
    ├── aarch64-unknown-linux-gnu/release/travel-net
    ├── armv7-unknown-linux-gnueabihf/release/travel-net
    ├── x86_64-unknown-linux-gnu/release/travel-net
    └── travel-net_0.1.0-1_*.deb
```

## Source Architecture

### main.rs orchestration order
1. Load config
2. Detect WiFi backend (NetworkManager or WpaSupplicant)
3. Auto-connect STA (if `sta_ssid` configured)
4. Start AP (`ap::start_ap` — dispatches to hostapd or NM backend)
5. If NOT NM backend: start dnsmasq + apply nftables
6. Start web UI on port 80

### Backend auto-detection (`wifi::detect_backend`)
- Checks for `/usr/bin/nmcli` and `/usr/sbin/NetworkManager`
- Returns `Backend::NetworkManager` or `Backend::WpaSupplicant`
- Determines: STA connect method, AP method, whether dnsmasq/nftables run

### hostapd path (brcmfmac devices: NanoPi, RPi)
1. `ap::interface::create_ap_interface()` — `iw phy phy0 interface add wlan1 type __ap`
2. `ap::assign_ap_ip()` — flush + assign `192.168.4.1/24`
3. `ap::hostapd::start_hostapd()` — write config, spawn `hostapd -B`
4. `dhcp::start_dnsmasq()` — write config, spawn dnsmasq
5. `firewall::apply_ruleset()` — nftables + IP forwarding

### NM path (AIC8800 devices: Cubie A5E/A7A)
1. `ap::networkmanager::start_nm_ap()` — create wlan1, assign IP, `nmcli connection add mode ap ifname wlan1 ipv4.method shared`
2. dnsmasq/nftables skipped entirely (NM shared mode handles DHCP + NAT)

### IP forwarding
- Set by `firewall/mod.rs` at startup: `sysctl -w net.ipv4.ip_forward=1`
- Persistent via `/etc/sysctl.d/99-travel-net.conf`
- Without it, forwarded packets are silently dropped

### AP subnet
- Default: `192.168.4.1/24`
- DHCP pool: `192.168.4.10` – `192.168.4.250`
- Captive portal redirect removed — portal directly at `192.168.4.1`, internet works through AP

## Cross-Compilation

### Setup
- **Rust toolchain**: `~/.rustup/toolchains/stable-aarch64-apple-darwin/` (NOT Homebrew)
- **RUSTC**: `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc`
- **CARGO**: `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo`
- **zigbuild**: `~/.cargo/bin/cargo-zigbuild`
- **Targets installed**: `aarch64-unknown-linux-gnu` (arm64), `armv7-unknown-linux-gnueabihf` (armhf), `x86_64-unknown-linux-gnu` (amd64)

### Build commands
```bash
cd /Users/Shared/Radxa_Cubie_A7A/travel-net
RUSTC=~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc \
CARGO=~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo \
~/.cargo/bin/cargo-zigbuild zigbuild --release --target aarch64-unknown-linux-gnu

# armhf
RUSTC=... ~/.cargo/bin/cargo-zigbuild zigbuild --release --target armv7-unknown-linux-gnueabihf

# amd64
RUSTC=... ~/.cargo/bin/cargo-zigbuild zigbuild --release --target x86_64-unknown-linux-gnu
```

### Install missing target
Rustup not in PATH, toolchain binary at:
```bash
ls ~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/
```
Download `.rust-std-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz` from `https://static.rust-lang.org/dist/` and extract into `lib/rustlib/` to add a target manually.

## Debian Packaging

### Build .deb
```bash
dpkg-deb -Zgzip --root-owner-group --build <dir> <output.deb>
```

### Package metadata
- **Control fields**: Package: travel-net, Version: 0.1.0-1
- **hostapd backend deps**: hostapd, dnsmasq, wpasupplicant, nftables, iw
- **NM backend deps**: network-manager, wpasupplicant, iw
- **Architectures**: arm64, armhf, amd64

### APT repo update
1. Clone `gh-pages` branch
2. Copy `.deb` files to repo root
3. Generate `Packages` + `Packages.gz` (manual flat-file format)
4. Generate `Release`
5. Commit + push

## Deployment

### NanoPi NEO Air (hostapd backend)
```bash
# Copy .deb
sshpass -p raga@098 scp travel-net_0.1.0-1_armhf.deb root@192.168.200.114:/tmp/

# Install
ssh root@192.168.200.114 "dpkg -i /tmp/travel-net_0.1.0-1_armhf.deb && apt install -f -y"

# Edit config for channel 4 (matching STA channel — brcmfmac constraint)
# Enable+start service
systemctl enable --now travel-net

# Disable conflicting services
systemctl disable nano-ap-init 2>/dev/null || true
systemctl disable nano-wifi-portal 2>/dev/null || true

# Set up mDNS
apt install avahi-daemon
cat > /etc/avahi/services/travel-net.service << 'EOF'
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name replace-wildcards="yes">Travel-Net Router</name>
  <service>
    <type>_http._tcp</type>
    <port>80</port>
  </service>
</service-group>
EOF

# Persistent IP forwarding
echo "net.ipv4.ip_forward=1" > /etc/sysctl.d/99-travel-net.conf
```

### Cubie A5E (NM backend)
```bash
# Copy binary directly (not .deb — scp binary, set up manually)
sshpass -p radxa scp travel-net radxa@192.168.1.35:/tmp/

ssh radxa@192.168.1.35
sudo cp /tmp/travel-net /usr/local/bin/
sudo chmod 755 /usr/local/bin/travel-net

# Config (write /etc/travel-net/config.json)
{
  "ap_interface": "wlan1",
  "sta_interface": "wlan0",
  "ap_ssid": "Travel-Net",
  "ap_password": "travelnet",
  "ap_channel": 6,
  "ap_ip": "192.168.4.1/24",
  "dnsmasq_conf": "/etc/travel-net/dnsmasq.conf",
  "nftables_conf": "/etc/travel-net/travel-net.nft",
  "hostapd_conf": "/etc/travel-net/hostapd.conf"
}

# systemd service
cat > /lib/systemd/system/travel-net.service << 'EOF'
[Unit]
Description=Travel Net NAT Router
After=network.target network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/travel-net
ExecStopPost=/bin/sh -c "pidof hostapd && killall hostapd; pidof dnsmasq && killall dnsmasq"
Restart=on-failure
RestartSec=5
User=root

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload && systemctl enable --now travel-net

# Passwordless sudo for travel-net
echo "radxa ALL=(ALL) NOPASSWD:/usr/local/bin/travel-net" > /etc/sudoers.d/travel-net
```

### Cubie A7A (NM backend — NOT YET DEPLOYED)
Follow Cubie A5E steps. Connect via 192.168.200.189. May need to stop old Python Flask portal first.

## GitHub Token
- Stored in macOS keychain/token variable — **DO NOT hardcode in files**
- Used in clone URLs: `https://ragavellur:${TOKEN}@github.com/ragavellur/rust-travel-router.git`
- If push is rejected with "secret detected", check if any tracked file contains the literal token string

## Critical Gotchas

### brcmfmac (NanoPi) channel constraint
AP and STA must be on the **same channel**. brcmfmac firmware doesn't support channel switch between virtual interfaces. If STA connects to a 5GHz network, AP (which is 2.4GHz-only) will fail. Current config: channel 4 (matching the STA's connected network on channel 4).

### AIC8800 hostapd failure
AIC8800 SDIO (A5E) and USB (A7A) both fail hostapd with "Failed to set beacon parameters". The `type __ap` interface creates successfully but hostapd can't connect to kernel driver. Workaround: use NM hotspot on a `type managed` virtual interface instead.

### Interface creation differences
- **NanoPi**: `iw phy phy0 interface add wlan1 type __ap` — creates in AP mode
- **Cubie A5E**: `iw phy phy0 interface add wlan1 type managed` — creates in managed mode; NM changes it to AP mode internally
- A5E's AIC8800 supports both `type __ap` and `type managed` for creation; NM backend uses `type managed`

### IP forwarding MUST be set
Kernel default is `net.ipv4.ip_forward=0` on Debian. Without it, all forwarded AP→Internet packets are dropped. Set at runtime in firewall module and persistently via `/etc/sysctl.d/99-travel-net.conf`.

### dnsmasq "unknown interface" race
If dnsmasq starts before the AP interface has its IP assigned, it fails with "unknown interface wlan1" and exits. The NM backend avoids this entirely by using NM's shared mode instead of dnsmasq.

### NetworkManager interference with hostapd
On NM devices (Cubie A5E/A7A), NM manages all interfaces by default. The NM backend handles this correctly. But if an NM device runs the hostapd path, NM may interfere with wlan1. Unlikely in practice since the hostapd path is only used on wpa_supplicant devices.

### APT repo is flat-file (not pool/ structure)
The gh-pages branch has `.deb` files at root, NOT in `pool/main/`. The `Packages` file is generated manually. `apt-ftparchive` may not work with flat repos — use manual generation.

## File Locations

| File | Location |
|------|----------|
| Rust source | `/Users/Shared/Radxa_Cubie_A7A/travel-net/src/` |
| Templates | `/Users/Shared/Radxa_Cubie_A7A/travel-net/templates/` |
| arm64 binary | `.../target/aarch64-unknown-linux-gnu/release/travel-net` |
| armhf binary | `.../target/armv7-unknown-linux-gnueabihf/release/travel-net` |
| amd64 binary | `.../target/x86_64-unknown-linux-gnu/release/travel-net` |
| .deb output | `.../target/travel-net_0.1.0-1_*.deb` |
| Config (NanoPi) | `/etc/travel-net/config.json` |
| Config (A5E) | `/etc/travel-net/config.json` |
| service unit | `/lib/systemd/system/travel-net.service` |
| mDNS service | `/etc/avahi/services/travel-net.service` |
| IP forwarding | `/etc/sysctl.d/99-travel-net.conf` |
| nftables rules | `/etc/travel-net/travel-net.nft` |
| hostapd config | `/etc/hostapd/travel-net.conf` |
| dnsmasq config | `/etc/travel-net/dnsmasq.conf` |

## Config Reference

```json
{
  "ap_ssid": "Travel-Net",
  "ap_password": "travelnet",
  "ap_ip": "192.168.4.1/24",
  "ap_netmask": "255.255.255.0",
  "ap_channel": 6,
  "ap_interface": "wlan1",
  "sta_interface": "wlan0",
  "dhcp_start": "192.168.4.100",
  "dhcp_end": "192.168.4.200",
  "web_password": "",
  "hostname": "travel-router",
  "sta_ssid": "",
  "sta_password": "",
  "wifi_backend": ""
}
```

**Note**: `ap_ip` should be in CIDR format (`192.168.4.1/24`) for NM backend. The hostapd backend uses separate `ap_ip` + `ap_netmask` fields.

## Common Tasks

### Restart travel-net on device
```bash
sudo systemctl restart travel-net
journalctl -u travel-net -f
```

### Check AP status
```bash
iw dev wlan1 info
hostapd_cli status   # hostapd backend only
nmcli connection show --active  # NM backend
```

### Check STA status
```bash
iw dev wlan0 link
nmcli device wifi list
```

### Update to latest code
```bash
# Build + copy binary, then:
sudo systemctl stop travel-net
sudo cp /tmp/travel-net /usr/local/bin/
sudo systemctl start travel-net
```

### Scan for WiFi
```bash
iw dev wlan0 scan | grep -E "SSID|freq|signal"
```

### Rebuild all .deb + update APT repo
```bash
# Build all 3 targets, rebuild 3 .deb files
# Clone gh-pages, replace .deb files, regenerate Packages + Packages.gz + Release
# Commit + push gh-pages
```

## Previous Goals (Completed)
- [x] NanoPi NEO Air travel-net deployment (hostapd backend)
- [x] Cubie A5E travel-net deployment (NM backend)
- [ ] Cubie A7A port from Python Flask to travel-net (NM backend)
- [x] amd64 .deb + APT repo support
- [x] Dashboard "Loading..." bug fix
- [x] Scan page with background polling (no meta refresh)
- [x] Shutdown button (POST /api/shutdown)
- [x] IP assignment in Rust code (assign_ap_ip)
- [x] IP forwarding enabled at startup
- [x] mDNS discovery publishing
- [x] 3-arch APT repo on GitHub Pages

## Hardware IPs Quick Reference

| Device | IP | User | Password | Arch | Backend |
|--------|-----|------|----------|------|---------|
| NanoPi NEO Air | 192.168.200.114 | root | raga@098 | armhf | hostapd |
| Cubie A5E | 192.168.1.35 | radxa | radxa | arm64 | NM |
| Cubie A7A | 192.168.200.189 | radxa | radxa | arm64 | NM (not yet) |
