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
| Radxa Cubie A7A | AIC8800D80 (USB) | arm64 | ✓ confirmed |

Requirements:
- Two WiFi interfaces (one for AP, one for STA), OR a single interface that supports virtual interfaces (AP on wlan1, STA on wlan0)
- `hostapd`, `dnsmasq`, `wpa_supplicant`, `nftables`, `iw` installed
- For AP+STA on single-radio chips (like brcmfmac): both interfaces must share the same channel

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

**arm64** (64-bit ARM, e.g. Radxa Cubie A7A, RPi 4/5):

```bash
cargo zigbuild --release --target aarch64-unknown-linux-gnu
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

## License

MIT
