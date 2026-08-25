# travel-net

**Single-binary travel NAT router** — turn any Linux SBC with WiFi into a portable NAT router with a web UI. Inspired by the [ESP32 NAT Router](https://github.com/martin-ger/esp32_nat_router).

## Table of Contents

- [Features](#features)
- [Architecture](#architecture)
- [Hardware Requirements](#hardware-requirements)
- [Installation](#installation)
- [Uninstall](#uninstall)
- [Configuration](#configuration)
- [VPN (WireGuard & Tailscale)](#vpn-wireguard--tailscale)
- [Usage](#usage)
- [Building from Source](#building-from-source)
- [Development](#development)
- [Optional: Read-Only Root Filesystem](#optional-read-only-root-filesystem)
- [License](#license)

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
- **VPN** — WireGuard or Tailscale, home + travel roles, kill switch (see [VPN setup guide](docs/VPN_GUIDE.md))

## Architecture

```
    ┌────────────────────────┐      ┌────────────────────────┐
    │         Web UI         │      │       axum HTTP        │
    │       (embedded)       ├─────▶│       (port 80)        │
    │      HTML / CSS        │      │                        │
    └────────────────────────┘      └───────────┬────────────┘
                                                │
                                                ▼
                                    ┌────────────────────────┐
                                    │    Backend Modules     │
                                    │  config · api · apply  │
                                    │       templates        │
                                    └───────────┬────────────┘
                                                │
        ┌──────────────┬──────────────┬───────┴──────┬──────────────┬──────────────┐
        │              │              │              │              │              │
        ▼              ▼              ▼              ▼              ▼              ▼
    ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐
    │    AP     │  │   STA /   │  │ Firewall  │  │    VPN    │  │  System   │  │   Other   │
    │ ┌───────┐ │  │   WiFi    │  │(nftables) │  │ ┌───────┐ │  │ (clients) │  │ (future)  │
    │ │hostapd│ │  │ ┌───────┐ │  │           │  │ │ Wire  │ │  │interfaces │  └───────────┘
    │ │ or NM │ │  │ │ wpa_  │ │  │ kill      │  │ │ Guard │ │  │  reboot   │
    │ └───────┘ │  │ │ supp  │ │  │ switch    │  │ └───────┘ │  │   logs    │
    │   DHCP    │  │ └───────┘ │  │ NAT /     │  │   kill    │  └───────────┘
    │ (dnsmasq) │  │  scan/con │  │ forward   │  │  switch   │
    └───────────┘  └───────────┘  └───────────┘  └───────────┘


```

- **Runtime**: single binary, zero runtime dependencies (not even Python)
- **Web framework**: [axum](https://github.com/tokio-rs/axum) on [tokio](https://tokio.rs)
- **Templates**: embedded into the binary at compile time via `include_str!`
- **Config**: JSON file at `/etc/travel-net/config.json`
- **AP backends**: `hostapd` + `dnsmasq` + `nftables` (brcmfmac devices: NanoPi/RPi), or NetworkManager hotspot (AIC8800 devices: Cubie A5E/A7A)
- **VPN**: WireGuard or Tailscale with home/travel roles and a kill switch (see [VPN setup guide](docs/VPN_GUIDE.md))

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
- Two WiFi interfaces (one for AP, one for STA), OR a single radio that supports virtual interfaces (AP on wlan1, STA on wlan0)
- **Note on single-radio USB dongles** (e.g. Realtek RTL8188FU/FTV, `rtl8xxxu`): these cannot create a second virtual interface, so the AP will not start while the same dongle is used as STA. travel-net still installs and connects to upstream Wi-Fi on them; use an Ethernet uplink (eth0) instead, or a radio with a supported driver (brcmfmac/AIC8800).
- `wpasupplicant` (or `network-manager`) and `iw` installed
- For **hostapd backend** (brcmfmac chips): `hostapd`, `dnsmasq`, `nftables` also required
- For **NM backend** (AIC8800 chips without hostapd support): `network-manager` handles AP, DHCP and NAT automatically
- AP+STA on single-radio chips (like brcmfmac): both interfaces must share the same channel

## Installation

### Fresh install (arm64 — Cubie A5E/A7A, RPi 4/5)

```bash
# 1. Sync clock first (prevents apt corruption on devices without RTC)
sudo timedatectl set-ntp true
sleep 10

# 2. Download and install (one command)
cd /tmp && wget https://ragavellur.github.io/rust-travel-router/travel-net_0.2.30-1_arm64.deb && sudo dpkg -i ./travel-net_0.2.30-1_arm64.deb

# 3. Reboot
sudo reboot
```

After reboot:
1. Connect to WiFi SSID **Travel-Net** (password: `travelnet`)
2. Open `http://192.168.4.1/` — the web dashboard shows status
3. Open `http://192.168.4.1/vpn` to configure VPN (Tailscale or WireGuard)

### Fresh install (armhf — NanoPi NEO Air, RPi 2/3/Zero 2)

```bash
cd /tmp && wget https://ragavellur.github.io/rust-travel-router/travel-net_0.2.30-1_armhf.deb && sudo dpkg -i ./travel-net_0.2.30-1_armhf.deb && sudo reboot
```

### Fresh install (amd64 — x86-64 laptops)

```bash
cd /tmp && wget https://ragavellur.github.io/rust-travel-router/travel-net_0.2.30-1_amd64.deb && sudo dpkg -i ./travel-net_0.2.30-1_amd64.deb && sudo reboot
```

### APT repository install

```bash
# 1. Add the repository
echo "deb [trusted=yes] https://ragavellur.github.io/rust-travel-router/ ./" | \
  sudo tee /etc/apt/sources.list.d/travel-net.list

# 2. Install
sudo apt-get update
sudo apt-get install -y travel-net
sudo reboot
```

> **Multi-arch systems** (e.g. 32-bit Raspberry Pi OS): pin the repo to your arch:
> ```bash
> echo "deb [trusted=yes arch=armhf] https://ragavellur.github.io/rust-travel-router/ ./" | \
>   sudo tee /etc/apt/sources.list.d/travel-net.list
> ```

## Uninstall

**Important:** Before uninstalling, connect to the device via Ethernet or uplink WiFi (NOT the Travel-Net AP). Uninstalling stops the AP and drops SSH if you're connected through it.

### Uninstall (one command)

```bash
sudo dpkg -r travel-net && sudo reboot
```

This stops all services, removes binaries, cleans up NM override, and removes stale files from old versions. The `prerm` script handles everything — no manual cleanup needed.

If you also want to remove the config and data:

```bash
sudo dpkg -r travel-net
sudo rm -rf /etc/travel-net /var/lib/travel-net
sudo reboot
```

## Configuration

Config file: `/etc/travel-net/config.json`

Edit manually or via the web UI at `http://192.168.4.1/config`.

```bash
sudo nano /etc/travel-net/config.json
sudo systemctl restart travel-net
```

| Field | Default | Description |
|-------|---------|-------------|
| `ap_ssid` | `Travel-Net` | Access point SSID |
| `ap_password` | `travelnet` | AP password (min 8 chars) |
| `ap_ip` | `192.168.4.1` | AP gateway IP |
| `ap_channel` | `0` | WiFi channel (0 = auto-detect from STA) |
| `ap_interface` | `wlan1` | Interface for AP mode |
| `sta_interface` | `wlan0` | Interface for STA mode |
| `web_password` | `""` | Web UI password (empty = no auth) |

## VPN (WireGuard & Tailscale)

Open `http://192.168.4.1/vpn` in the web UI.

### Tailscale (easiest)

1. Click **Install VPN Tools** on the VPN page (needs internet)
2. Select **Tailscale** as VPN type
3. Pick **Travel box** (goes with you) or **Home box** (stays at home)
4. Create a free [Tailscale account](https://login.tailscale.com/)
5. Generate an **auth key** in Tailscale admin → Settings → Keys
6. Paste the auth key, set a hostname
7. **Route all internet through home**: off = only home subnet routes via Tailscale, internet goes direct. On = all traffic via home.
8. Click **Save & Apply**

### WireGuard

See the [VPN setup guide](docs/VPN_GUIDE.md) for step-by-step instructions with both home and travel box configurations.

## Usage

Connect to the Travel-Net WiFi and open `http://192.168.4.1/`.

| Page | URL | Description |
|------|-----|-------------|
| Dashboard | `/` | Status, interfaces, clients |
| WiFi Scan | `/scan` | Scan nearby networks, connect |
| Configuration | `/config` | Edit AP/network settings |
| VPN | `/vpn` | WireGuard or Tailscale setup |
| Setup Wizard | `/setup` | Guided initial setup |
| Logs | `/logs` | System journal viewer |

### API

All API endpoints return JSON:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/status` | GET | System status (AP, STA, clients, uptime, interfaces) |
| `/api/scan` | GET | WiFi scan results |
| `/api/connect` | POST | Connect to a WiFi network (STA) |
| `/api/config` | GET/PUT | Read/write configuration |
| `/api/clients` | GET | List connected DHCP clients |
| `/api/clients/disconnect` | POST | Disconnect a client from the AP |
| `/api/clients/unblock` | POST | Unblock a previously blocked client |
| `/api/vpn` | GET | VPN config + live status (tunnel, handshake, kill switch) |
| `/api/vpn` | POST | Save + apply VPN settings |
| `/api/vpn/import` | POST | Parse a WireGuard `.conf` file into settings |
| `/api/vpn/genkeys` | POST | Generate a WireGuard keypair (client or server) |
| `/api/vpn/peers` | POST | Add/remove a travel device on the home box |
| `/api/vpn/install` | POST | Install missing VPN tools (WireGuard/Tailscale) |
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

- Rust 1.75+ with cross-compilation targets
- [cargo-zigbuild](https://github.com/benesch/cargo-zigbuild) (for cross-compilation)

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

### Build all .deb packages

```bash
./build-debs.sh
```

Produces `.deb` files in `target/debs/` for all three architectures.

## Development

```bash
# Build and run locally
cargo run -- --config config/etc/travel-net/config.json
```

## Optional: Read-Only Root Filesystem

Protect the SD card/eMMC from corruption when power is pulled abruptly. **Skip this section if you prefer a writable rootfs.**

```bash
# 1. Edit /etc/fstab — change the rootfs line from:
#      UUID=... / ext4 defaults,noatime 0 1
#    to:
#      UUID=... / ext4 defaults,noatime,ro 0 1
sudo nano /etc/fstab

# 2. Add these tmpfs lines at the bottom of /etc/fstab:
#     tmpfs   /tmp                       tmpfs   defaults,noatime,nosuid,nodev,mode=1777   0   0
#     tmpfs   /var/log                   tmpfs   defaults,noatime,nosuid,nodev,mode=0755   0   0
#     tmpfs   /var/tmp                   tmpfs   defaults,noatime,nosuid,nodev,mode=1777   0   0
#     tmpfs   /var/lib/NetworkManager    tmpfs   defaults,noatime,nosuid,nodev            0   0

# 3. Reboot to lock
sudo reboot
```

After reboot, verify:
```bash
cat /proc/mounts | grep ' / '          # Should show ro,noatime
cat /proc/mounts | grep tmpfs          # Should show tmpfs mounts
```

## License

MIT
