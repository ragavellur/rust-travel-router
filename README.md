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
- **VPN** — WireGuard or Tailscale, home + travel roles, kill switch (see [VPN setup guide](docs/VPN_GUIDE.md))

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
- Two WiFi interfaces (one for AP, one for STA), OR a single radio that supports virtual interfaces (AP on wlan1, STA on wlan0)
- **Note on single-radio USB dongles** (e.g. Realtek RTL8188FU/FTV, `rtl8xxxu`): these cannot create a second virtual interface, so the AP will not start while the same dongle is used as STA. travel-net still installs and connects to upstream Wi-Fi on them; use an Ethernet uplink (eth0) instead, or a radio with a supported driver (brcmfmac/AIC8800).
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

> **Multi-arch systems** (e.g. 32-bit Raspberry Pi OS, which enables `arm64` as a
> foreign architecture): pin the repo to your native arch so `apt` picks the
> right package instead of a foreign-arch one. Check with `dpkg --print-architecture`,
> then add `arch=<your-arch>` to the source line:
>
> ```bash
> # armhf (32-bit Pi, NanoPi)
> echo "deb [trusted=yes arch=armhf] https://ragavellur.github.io/rust-travel-router/ ./" | \
>   sudo tee /etc/apt/sources.list.d/travel-net.list
> # arm64 (Cubie A5E/A7A)
> # echo "deb [trusted=yes arch=arm64] https://ragavellur.github.io/rust-travel-router/ ./" | \
> #   sudo tee /etc/apt/sources.list.d/travel-net.list
> sudo apt update
> sudo apt install travel-net
> ```
>
> The repo hosts `armhf`, `arm64`, and `amd64` packages. The `armhf` package uses
> the hostapd backend (RPi, NanoPi); `arm64`/`amd64` use the NetworkManager backend.

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

### Uninstall

Stop the service, remove the package, then clean up leftover files.

#### Via APT package

```bash
sudo systemctl stop travel-net
sudo apt remove --purge travel-net
```

#### Via .deb package

```bash
sudo systemctl stop travel-net
sudo dpkg -r travel-net       # or: sudo dpkg -P travel-net (also purge config)
```

#### Manual install

Remove the binary, service unit and config:

```bash
sudo systemctl stop travel-net
sudo rm -f /usr/local/bin/travel-net /usr/sbin/travel-net
sudo rm -f /lib/systemd/system/travel-net.service /etc/systemd/system/travel-net.service
sudo rm -rf /etc/travel-net
sudo systemctl daemon-reload
```

#### Clean up leftovers

```bash
# APT source entry (if you added it)
sudo rm -f /etc/apt/sources.list.d/travel-net.list

# NetworkManager: forget the hotspot profile and any unmanaged-interface overrides
sudo nmcli connection delete Cubie-AP 2>/dev/null || true
sudo rm -f /etc/NetworkManager/conf.d/98-travel-net-unmanaged.conf
sudo rm -f /etc/NetworkManager/conf.d/unmanaged-wlan1.conf

# Remove any leftover AP interface
sudo iw dev wlan1 del 2>/dev/null || true

# Firewall: flush travel-net's nftables rules and reset IP forwarding
sudo nft flush ruleset
sudo sysctl -w net.ipv4.ip_forward=0
sudo rm -f /etc/sysctl.d/99-travel-net.conf
sudo rm -f /etc/modprobe.d/aic8800-travel-net.conf
```

> **Read-only rootfs note:** if you hardened the rootfs to `ro` (see the
> *Optional: Read-Only Root Filesystem* section), the service/config files above
> can only be deleted while the rootfs is unlocked (`sudo mount -o remount,rw /`).
> Also remove the `ro` option from `/etc/fstab` if you no longer want a locked
> rootfs.

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

**Steps (run on a writable rootfs, in order):**

```bash
# === PART 1: SYSTEM CONFIGURATION (one-time) ===

# 1. Allow NM to write hotspot profiles (overrides ProtectSystem=true in systemd unit)
sudo mkdir -p /etc/systemd/system/NetworkManager.service.d
sudo bash -c 'cat > /etc/systemd/system/NetworkManager.service.d/override.conf << "EOF"
[Service]
ReadWritePaths=/etc/NetworkManager/system-connections
EOF'

# 2. Ensure NetworkManager manages all interfaces
sudo sed -i 's/managed=false/managed=true/' /etc/NetworkManager/NetworkManager.conf

# 3. Remove ifupdown plugin (it blocks connection creation even with managed=true)
sudo sed -i 's/plugins=ifupdown,keyfile/plugins=keyfile/' /etc/NetworkManager/NetworkManager.conf

# 4. Disable NM periodic Wi-Fi scanning (prevents AP from dropping every 30s on single-radio)
sudo bash -c 'cat > /etc/NetworkManager/conf.d/90-no-periodic-scan.conf << "EOF"
[connectivity]
interval=0
[wifi]
scan-rand-mac-address=no
EOF'

# 5. Set dns=systemd-resolved so NM pushes DNS to systemd-resolved (needed for ro rootfs — NM can't write /etc/resolv.conf)
sudo sed -i '/^\[main\]/a dns=systemd-resolved' /etc/NetworkManager/NetworkManager.conf

# 6. Enable IP forwarding (one-time permanent)
echo "net.ipv4.ip_forward=1" | sudo tee /etc/sysctl.d/99-travel-net.conf

# 7. Reload systemd and restart NM to apply all changes
sudo systemctl daemon-reload
sudo systemctl restart NetworkManager

# === PART 2: NETWORK SETUP ===

# 8. Connect to upstream WiFi (STA mode) — reboots will auto-connect
sudo nmcli device wifi connect "YourSSID" password "YourPassword" ifname wlan0

# 9. Pre-create the NM hotspot profile
sudo nmcli connection add type wifi mode ap con-name travel-net-hotspot ifname wlan1 \
  ssid "Travel-Net" wifi.band a wifi-sec.key-mgmt wpa-psk wifi-sec.psk "travelnet" \
  ipv4.method shared ipv4.address 192.168.4.1/24

# 10. Pre-create static upstream DNS config for NM's internal dnsmasq
sudo mkdir -p /etc/NetworkManager/dnsmasq-shared.d
sudo bash -c 'cat > /etc/NetworkManager/dnsmasq-shared.d/01-upstream.conf << "EOF"
server=8.8.8.8
server=1.1.1.1
EOF'

# 11. Enable AP channel auto-detection (matches STA channel automatically)
sudo sed -i 's/"ap_channel": [0-9]*/"ap_channel": 0/' /etc/travel-net/config.json

# 12. Start travel-net
sudo systemctl enable --now travel-net
```

After travel-net starts, verify:
```bash
journalctl -u travel-net --no-pager -n 10  # AP should start
iw dev wlan1 info                      # Should show type AP, matching STA channel
host google.com 192.168.4.1            # DNS should resolve
```

**Why each step?**

| Step | Why |
|------|------|
| `ReadWritePaths` override | NM's Debian systemd unit has `ProtectSystem=true`, making `/etc` read-only inside NM. Without this, `nmcli connection add` fails with *"Read-only file system"* even on rw rootfs. |
| `managed=true` | Without it, the `ifupdown` plugin claims all interfaces and the `keyfile` plugin can't add hotspot connections. |
| `plugins=keyfile` | The `ifupdown` plugin causes *"settings plugin does not support adding connections"* even with `managed=true`. Unnecessary since this device doesn't use `/etc/network/interfaces`. |
| No periodic scan | AIC8800 is a single-radio chip — scanning on wlan0 stops the AP on wlan1 every 30s. Disabling periodic scans keeps the AP stable. |
| `dns=systemd-resolved` | NM pushes DNS to systemd-resolved instead of writing `/etc/resolv.conf`. Required on ro rootfs; recommended for all setups. |
| `ap_channel: 0` | AP and STA must share the same channel. travel-net auto-detects the STA's channel from `iw dev wlan0 link` and sets the AP to match. |
| Pre-created NM profile | Required on ro (NM can't write new profiles). Harmless on rw — travel-net uses read-only `nmcli connection up` instead of write. |
| Static dnsmasq upstream config | NM writes `server=` entries to dnsmasq-shared.d at runtime but can't on ro rootfs. Without upstream DNS, clients connect but can't browse. Pre-creating fixes this on both rw and ro. |

**Troubleshooting:**

| Symptom | Cause | Fix |
|---------|-------|------|
| `settings plugin does not support adding connections` | `ifupdown` plugin still in `plugins=` line | Change to `plugins=keyfile` and restart NM |
| `error writing ... Read-only file system` | Missing `ReadWritePaths` override | Check `/etc/systemd/system/NetworkManager.service.d/override.conf` exists |
| `No suitable device found` | NM hasn't discovered wlan1 yet | travel-net retries 5 times; manually: `nmcli connection up travel-net-hotspot` |
| SSID not visible | NM periodic scanning stopping the AP | Verify `90-no-periodic-scan.conf` exists; restart NM |
| `iw` command errors in logs | `iw` not in PATH | Already handled — travel-net uses `Command::new("iw")` which resolves via systemd PATH (`/sbin`) |
| AP channel doesn't match STA | STA wasn't connected when detection ran | travel-net retries 5s; if still disconnected, NM auto-selects. Both on 5GHz, acceptable. |
| Client connects then disconnects repeatedly | Only on ro rootfs — dnsmasq can't write DHCP leases | See [Optional: Read-Only Rootfs](#optional-read-only-root-filesystem) |
| Client connects but can't browse | Only on ro rootfs — dnsmasq has no upstream DNS | Verify step 9 completed. If missing, see [Optional: Read-Only Rootfs](#optional-read-only-root-filesystem) |
| A5E can't resolve hostnames | `/etc/resolv.conf` empty on ro rootfs | See [Optional: Read-Only Rootfs](#optional-read-only-root-filesystem) |

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

## VPN (WireGuard & Tailscale)

One travel-net box **stays at home**, another **travels with you**. When you
connect to the travel box's Wi-Fi, all your traffic goes through a private
tunnel back to your home box and out to the internet from home — so websites
see your home address, you can reach your home devices from anywhere, and your
data stays encrypted on public Wi-Fi.

Pick a VPN type:

| | Tailscale | WireGuard |
|---|---|---|
| Easy? | Very easy. | A few more steps. |
| Works anywhere? | Yes, no router changes. | Yes, but the home router needs UDP port 51820 forwarded. |
| Cost | Free (Personal plan, up to 100 devices). | Free, no account. |
| Who runs it? | Tailscale's service coordinates the tunnel. | You. No third party. |

- **Non-technical users:** follow the [VPN setup guide (plain English)](docs/VPN_GUIDE.md) — step by step, with pictures in your head. It covers both a two-travel-net-box setup and a travel box paired with an existing home computer (Raspberry Pi / laptop).
- **Developers:** see [VPN technical design](docs/VPN_TECHNICAL.md) for the full architecture (config schema, nftables integration, API, pairing flows).

Quick pointers:

- Open the **VPN** page in the web console (or `http://192.168.4.1/vpn`).
- Pick whether this box is the **home box** (stays at home, always on) or the **travel box** (goes with you), then pick a VPN service.
- **Install VPN tools** on the VPN page once (needs internet). The services stay off until you save a VPN.
- WireGuard help is built into the page: the home box can **generate keys**, **add travel devices** and show a ready-to-paste config for each; the travel box can **paste a config file** or fill the fields by hand.
- Tailscale: create a free account, generate an **auth key**, and paste it in — the page walks you through the rest.

> **VPN config is stored** in the same `/etc/travel-net/config.json` file, under the `vpn` key. Manual edits there are picked up after a `systemctl restart travel-net`.

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
| VPN | `/vpn` | Set up WireGuard or Tailscale (home/travel) |
| Connected Clients | `/clients` | List, block, unblock connected devices |
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

## Optional: Read-Only Root Filesystem

Protect the SD card/eMMC from corruption when power is pulled abruptly. **Skip this section if you prefer a writable rootfs.** The main deployment guide above (steps 1-11) works on both rw and ro — this just locks it down.

**Prerequisite:** Complete steps 1-12 from the deployment guide first (travel-net must be running).

```bash
# === LOCK ROOTFS READ-ONLY ===

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

# 3. Update NM's systemd override to add read-write paths for ro rootfs:
sudo sed -i '/ReadWritePaths=\/etc\/NetworkManager\/system-connections/a ReadWritePaths=/var/lib/NetworkManager' \
  /etc/systemd/system/NetworkManager.service.d/override.conf

# 4. Symlink /etc/resolv.conf to systemd-resolved stub (NM can't write it on ro)
sudo ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf

# 5. Reboot to lock
sudo reboot
```

After reboot, verify:
```bash
cat /proc/mounts | grep ' / '          # Should show ro,noatime
cat /proc/mounts | grep tmpfs          # Should show tmpfs mounts
journalctl -u travel-net --no-pager -n 10  # AP should start
host google.com 192.168.4.1            # DNS should resolve
```

### Making changes later

```bash
sudo mount -o remount,rw /     # unlock (may fail if services hold files open)
# ... make edits or install packages ...
sudo mount -o remount,ro /     # re-lock (may fail if services hold files open)
sudo reboot                    # guaranteed clean ro boot
```

## License

MIT
