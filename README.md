# travel-net

**Single-binary travel NAT router** — turn any Linux SBC with WiFi into a portable NAT router with a web UI. Inspired by the [ESP32 NAT Router](https://github.com/martin-ger/esp32_nat_router).

## Features

- **AP + STA hybrid mode** — simultaneously act as a WiFi access point (clients connect to you) and a station (you connect to upstream WiFi)
- **Web dashboard** — real-time status, interface info, connected clients, logs
- **WiFi scanning** — discover nearby networks and connect to them
- **Config management** — change SSID, password, channel, DHCP range via web UI
- **Persistent STA** — connect to a WiFi network and it auto-reconnects on boot
- **Optional auth** — password-protect the web UI
- **Factory reset** — one-click reset to defaults
- **Log viewer** — browse systemd journal from the UI
- **Debian packaging** — install via `.deb` or `apt`

## Architecture

```
┌────────────┐    ┌──────────────┐    ┌─────────────┐
│  Web UI    │───▶│  axum HTTP   │───▶│  Backend    │
│ (embedded  │    │  (port 80)   │    │  Modules    │
│  HTML/CSS) │    └──────────────┘    └──────┬──────┘
└────────────┘                               │
                    ┌────────────────────────┼────────────────────┐
                    │                        │                    │
               ┌────▼────┐           ┌───────▼───────┐    ┌──────▼──────┐
               │  AP     │           │  STA/WiFi     │    │  System     │
               │(hostapd)│           │(wpa_supplicant)│    │(clients,    │
               │  DHCP   │           │  Scan/Connect │    │ interfaces, │
               │(dnsmasq)│           │               │    │ reboot,     │
               │ Firewall│           │               │    │ uptime,     │
               │(nftables)│          │               │    │ logs)       │
               └─────────┘           └───────────────┘    └─────────────┘
```

- **Runtime**: single binary, zero runtime dependencies (not even Python)
- **Web framework**: [axum](https://github.com/tokio-rs/axum) on [tokio](https://tokio.rs)
- **Templates**: embedded into the binary at compile time via `include_str!`
- **Config**: JSON file at `/etc/travel-net/config.json`

## Hardware Requirements

Any Linux SBC with at least one WiFi interface. Tested on:

| Device | WiFi Chip | Arch | Works |
|--------|-----------|------|-------|
| NanoPi NEO Air | BCM43430 (brcmfmac) | armhf | ✓ confirmed |
| Raspberry Pi Zero 2 W | BCM43438 (brcmfmac) | armhf | ✓ (same driver as NanoPi) |
| Raspberry Pi 3 | BCM43438 (brcmfmac) | armhf/arm64 | ✓ expected |
| Raspberry Pi 4 | BCM43455 (brcmfmac) | arm64 | ✓ expected |
| Raspberry Pi 5 | BCM43455 / RP1 | arm64 | ✓ expected |
| Radxa Cubie A7A | AIC8800D80 (USB, dual MAC) | arm64 | ✓ confirmed (NM backend) |
| Radxa Cubie A5E | AIC8800D80 (SDIO, single MAC) | arm64 | ✓ confirmed (NM backend) |
| Any x86-64 laptop | any | amd64 | ✓ (NM backend) |

Requirements:
- Two WiFi interfaces (one for AP, one for STA), OR a single interface that supports virtual interfaces (AP on wlan1, STA on wlan0)
- `wpasupplicant` (or `network-manager`) and `iw` installed
- For **hostapd backend** (brcmfmac chips): `hostapd`, `dnsmasq`, `nftables` also required
- For **NM backend** (AIC8800 chips without hostapd support): `network-manager` handles AP, DHCP and NAT automatically
- AP+STA on single-radio chips (like brcmfmac): both interfaces must share the same channel

## Installation

### Via APT (recommended)

```bash
# Add the repository
echo "deb [trusted=yes] https://ragavellur.github.io/rust-travel-router/ ./" | \
  sudo tee /etc/apt/sources.list.d/travel-net.list

# Install
sudo apt update
sudo apt install travel-net
```

### Via .deb package

Download the latest `.deb` from [releases](https://github.com/ragavellur/rust-travel-router/releases) and install:

```bash
sudo dpkg -i travel-net_*.deb
sudo apt install -f   # install missing dependencies
```

### Installation troubleshooting

If `sudo apt install travel-net` fails with a hash or size mismatch like:

```
Err:1 https://ragavellur.github.io/rust-travel-router ./ travel-net 0.1.0-1
  File has unexpected size (1114530 != 1097722). Mirror sync in progress?
  Hashes of expected file:
   - SHA256:3a4c27b9...
   - Filesize:1097722
```

The APT cache is stale. Clear it and retry:

```bash
sudo rm -rf /var/lib/apt/lists/ragavellur.github.io*
sudo apt update
sudo apt install travel-net
```

### Post-install

Edit the config to match your hardware:

```bash
sudo nano /etc/travel-net/config.json
```

Then start the service:

```bash
sudo systemctl enable --now travel-net
```

## Deployment Guides

### NanoPi NEO Air & brcmfmac devices (hostapd backend)

For Broadcom WiFi chips (NanoPi NEO Air, Raspberry Pi), the traditional hostapd + dnsmasq + nftables backend is used.

**Steps:**

```bash
sudo apt install -y hostapd dnsmasq nftables

# Set AP channel to match STA's channel (required — single radio constraint)
# Find STA channel: iw dev wlan0 link
sudo sed -i 's/"ap_channel": 0/"ap_channel": 4/' /etc/travel-net/config.json

sudo systemctl disable --now dnsmasq hostapd   # travel-net manages its own

# Enable IP forwarding (one-time)
echo "net.ipv4.ip_forward=1" | sudo tee /etc/sysctl.d/99-travel-net.conf

sudo systemctl enable --now travel-net
```

**Channel constraint**: brcmfmac firmware does not support channel switching between virtual interfaces on the same phy. The AP (wlan1) and STA (wlan0) **must** be on the same channel. Set `ap_channel` to match the STA's current channel.

**mDNS discovery:**
```bash
sudo apt install avahi-daemon
cat > /tmp/travel-net.service << 'SVCEOF'
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name replace-wildcards="yes">Travel-Net Router</name>
  <service>
    <type>_http._tcp</type>
    <port>80</port>
  </service>
</service-group>
SVCEOF
sudo mv /tmp/travel-net.service /etc/avahi/services/
sudo systemctl enable --now avahi-daemon
```

### Radxa Cubie A5E/A7A & AIC8800 devices (NM backend)

For AIC8800 WiFi chips (Radxa Cubie A5E SDIO, Cubie A7A USB), `hostapd` fails with *"Failed to set beacon parameters"*. The NetworkManager hotspot backend is required instead.

**Steps:**

```bash
# 1. Ensure NetworkManager manages all interfaces (critical!)
sudo sed -i 's/managed=false/managed=true/' /etc/NetworkManager/NetworkManager.conf
sudo systemctl restart NetworkManager

# 2. Disable conflicting services
sudo systemctl disable --now dnsmasq hostapd 2>/dev/null || true

# 3. Enable STA auto-connect (connect to upstream WiFi on boot)
sudo nmcli device wifi connect "YourSSID" password "YourPassword" ifname wlan0

# 4. Enable AP channel auto-detection in config
sudo sed -i 's/"ap_channel": [0-9]*/"ap_channel": 0/' /etc/travel-net/config.json

# 5. Enable IP forwarding (one-time)
echo "net.ipv4.ip_forward=1" | sudo tee /etc/sysctl.d/99-travel-net.conf

# 6. Start travel-net
sudo systemctl enable --now travel-net
```

**Why `managed=true`?** With `managed=false` (Debian default), NetworkManager's `ifupdown` plugin claims all interfaces before the `keyfile` plugin runs. Creating new hotspot connections via `nmcli connection add` fails with *"settings plugin does not support adding connections"*. Setting `managed=true` fixes this.

**Why `ap_channel: 0`?** AIC8800 is a single-radio chip. The AP and STA share the same phy and must operate on the same frequency. When `ap_channel` is 0, travel-net reads the STA's current channel from `iw dev wlan0 link` and configures the AP to match. Without this, the AP may end up on a different band (e.g., 2.4GHz while STA is on 5GHz), halving throughput and adding latency.

**Troubleshooting NM backend:**

| Symptom | Cause | Fix |
|---------|-------|-----|
| `settings plugin does not support adding connections` | `managed=false` in NetworkManager.conf | Set `managed=true`, restart NM |
| `No suitable device found for this connection` | NM hasn't discovered wlan1 yet travel-net includes a retry loop, but after 5 attempts it still fails if NM is slow | Increase NM activation retries in config, or manually: `nmcli connection up travel-net-hotspot` |
| AP on wrong channel (e.g., 2.4GHz when STA is on 5GHz) | STA channel detection failed (wlan0 wasn't connected yet at boot) | Check `iw dev wlan0 link` — if "Not connected", NM auto-connect may be delayed. travel-net retries detection for 5 seconds |
| SSID not visible | wlan1 not created or NM hotspot not activated | Run `iw dev wlan1 info` — if `type managed` instead of `type AP`, the hotspot activation failed. Check `journalctl -u travel-net -n 20` |
| `iw` command not found in systemd service | `/sbin/iw` not in service PATH | Already handled — travel-net uses `Command::new("iw")` which resolves via PATH |

**Read-only root filesystem (eMMC/SD card protection):**

See [Hardening section](#hardening-read-only-root-filesystem) below. When using read-only root, remount rw for changes:

```bash
sudo mount -o remount,rw /
# ... make changes ...
sudo mount -o remount,ro /
```

## Configuration

Config file: `/etc/travel-net/config.json`

```json
{
  "ap_ssid": "Travel-Net",
  "ap_password": "travelnet",
  "ap_ip": "192.168.4.1",
  "ap_netmask": "255.255.255.0",
  "ap_channel": 6,
  "ap_interface": "wlan1",
  "sta_interface": "wlan0",
  "dhcp_start": "192.168.4.100",
  "dhcp_end": "192.168.4.200",
  "web_password": "",
  "hostname": "travel-router"
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `ap_ssid` | `Travel-Net` | Access point SSID |
| `ap_password` | `travelnet` | AP password (min 8 chars) |
| `ap_ip` | `192.168.4.1` | AP gateway IP |
| `ap_netmask` | `255.255.255.0` | AP subnet mask |
| `ap_channel` | `6` | WiFi channel (1-11 for 2.4GHz) |
| `ap_interface` | `wlan1` | Interface for AP mode |
| `sta_interface` | `wlan0` | Interface for STA mode |
| `dhcp_start` | `192.168.4.100` | DHCP pool start |
| `dhcp_end` | `192.168.4.200` | DHCP pool end |
| `web_password` | `""` | Web UI password (empty = no auth) |
| `hostname` | `travel-router` | System hostname |

## Usage

### Web UI

Connect to the AP SSID and open `http://192.168.4.1/` in a browser.

#### mDNS Discovery

If avahi-daemon is installed (recommended), the router publishes itself via mDNS. Install it with:

```bash
sudo apt install avahi-daemon
sudo systemctl enable --now avahi-daemon
```

Once running, open these URLs in a browser instead of remembering the IP:

- `http://travel-router.local/` — from the AP side (same network as clients)
- `http://nanopi-neo-air.local/` — from the upstream LAN (if connected via STA)

*Note: mDNS is typically `.local` only works on Linux/macOS. Windows needs Bonjour installed, Android needs a third-party app.*

| Page | Route | Description |
|------|-------|-------------|
| Dashboard | `/` | Status, interfaces, clients |
| WiFi Scan | `/scan` | Scan nearby networks, connect |
| Configuration | `/config` | Edit AP/network settings |
| Setup Wizard | `/setup` | Guided initial setup |
| Logs | `/logs` | System journal viewer |
| Login | `/login` | Auth page (if password is set) |

### API

All API endpoints return JSON:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/status` | GET | System status (AP, STA, clients, uptime, interfaces) |
| `/api/scan` | GET | WiFi scan results |
| `/api/connect` | POST | Connect to a WiFi network (STA) |
| `/api/config` | GET/PUT | Read/write configuration |
| `/api/clients` | GET | List connected DHCP clients |
| `/api/reboot` | POST | Reboot the device |
| `/api/reset` | POST | Factory reset |
| `/api/logs` | GET | Recent system logs |
| `/api/login` | POST | Authenticate for the web UI |

### CLI

```bash
# Show status
travel-net --help
```

## Building from Source

### Prerequisites

- Rust 1.75+ with `armv7-unknown-linux-gnueabihf` target (for cross-compilation)
- [cargo-zigbuild](https://github.com/benesch/cargo-zigbuild) (for cross-compilation)

### Native Build

```bash
git clone https://github.com/ragavellur/rust-travel-router.git
cd rust-travel-router
cargo build --release
```

### Cross-compilation

**armhf** (32-bit ARM, e.g. NanoPi NEO Air, RPi 2/3/Zero 2):

```bash
cargo zigbuild --release --target armv7-unknown-linux-gnueabihf
```

**arm64** (64-bit ARM, e.g. Radxa Cubie A7A/A5E, RPi 4/5):

```bash
cargo zigbuild --release --target aarch64-unknown-linux-gnu
```

**amd64** (x86-64 laptops):

```bash
cargo zigbuild --release --target x86_64-unknown-linux-gnu
```

### Build .deb package

On a Debian system with `dpkg-dev`:

```bash
make deb
```

Or manually (cross-compile):

```bash
./cross-build.sh
```

## Development

```bash
# Build and run locally
cargo run -- --config config/etc/travel-net/config.json
```

The `config.json` is pre-configured for a local test environment. Adjust interfaces as needed.

## Hardening: Read-Only Root Filesystem

Protect the SD card/eMMC from corruption when power is pulled abruptly:

<details>
<summary>Click to expand steps</summary>

**1. Open your filesystem configuration file**
```bash
sudo nano /etc/fstab
```

**2. Find your third line that looks like this:**
```
UUID=7cf9f135-82db-4514-af4e-5817cb922fec / ext4 defaults 0 1
```

**3. Change "defaults" to "defaults,noatime,ro"**
```
UUID=7cf9f135-82db-4514-af4e-5817cb922fec / ext4 defaults,noatime,ro 0 1
```

**4. Paste these lines at the absolute bottom to force system writes into RAM:**
```
tmpfs   /tmp        tmpfs   defaults,noatime,nosuid,nodev,mode=1777   0   0
tmpfs   /var/log    tmpfs   defaults,noatime,nosuid,nodev,mode=0755   0   0
tmpfs   /var/tmp    tmpfs   defaults,noatime,nosuid,nodev,mode=1777   0   0
```

**5. Save and exit** (Ctrl+O, Enter, Ctrl+X)

**6. Reboot the board to lock it down safely**
```bash
sudo reboot
```

### Making changes later

```bash
# 1. UNLOCK the system to make configuration edits or install packages
sudo mount -t ext4 -o remount,rw /

# [ Make your travel router changes, update Wi-Fi, or tweak settings here ]

# 2. LOCK the system back down immediately without needing a reboot
sudo mount -o remount,ro /
```

</details>

## License

MIT
