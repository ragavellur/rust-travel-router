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

To install packages or edit configs, remount root as read-write:
```bash
sudo mount -o remount,rw /
```

When done, re-lock:
```bash
sudo mount -o remount,ro /
```

Or simply reboot — it always boots back into read-only mode.

</details>

## License

MIT
