# travel-net VPN Feature — Technical Implementation Document

Status: **Draft / to be implemented** — tracked in [`TASKS.json`](../TASKS.json)

## 1. Purpose

Add a NomadEOS-style VPN capability to travel-net: two boxes, one that **stays home** and one that **travels**. Every device connected to the travel box's AP tunnels through the home box, so:

- the internet sees the **home IP** (not the hotel/4G IP),
- travel devices can **reach home LAN devices** (NAS, cameras, printers),
- corporate/enterprise VPNs and geo-restricted services see a home address.

Two backends are supported and selectable per box from the web console: **Tailscale** (works behind any NAT, no port forwarding) and **WireGuard** (direct tunnel, esp32_nat_router-style).

**Users may be totally non-technical.** All decisions below are driven by that: defaults are safe, help text is plain language, and the WireGuard home-server setup is a guided two-sided flow with copy-paste instructions for the case where the home side is an *existing* machine (not a travel-net box).

## 2. Design decisions (agreed)

1. **Tailscale is installed as part of package installation** (when travel-net is installed, all dependencies are installed) **but is NOT enabled** — `tailscaled` is installed but does not auto-start until the user configures the VPN in the web console.
2. **WireGuard home-server pairing** is a guided two-sided flow:
   - Travel box generates its keypair and shows its public key.
   - Home box (travel-net) adds it as a peer and shows a ready-to-paste client config.
   - **For an existing machine/server at home, the exact equivalent steps are documented and shown in the web-console help** so a non-technical user can follow them on a normal Linux machine/Raspberry Pi.
3. All VPN config is stored in the existing `/etc/travel-net/config.json` (nested `vpn` object). No schema migration needed (serde defaults).
4. VPN NAT lives in a **dedicated nft table** owned by the VPN module so it works on both the hostapd backend and the NetworkManager backend (NM boxes currently have no ruleset).

## 3. Config schema — `src/config.rs`

Add a nested struct so the existing flat `/config` form and `ConfigUpdate` are untouched:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnConfig {
    // common
    pub backend: String,              // "none" | "tailscale" | "wireguard"
    pub role: String,                 // "home" | "travel"

    // ---- Tailscale ----
    pub ts_auth_key: String,          // from tailscale.com admin console
    pub ts_hostname: String,          // default "<hostname>-ts"
    pub ts_exit_node: String,         // travel: home node name or IP
    pub ts_advertise_routes: String,  // home: home LAN CIDR, e.g. "192.168.1.0/24"
    pub ts_allow_lan: bool,           // travel: keep hotel LAN reachable (default true)

    // ---- WireGuard (client / travel) ----
    pub wg_private_key: String,
    pub wg_peer_public_key: String,
    pub wg_endpoint: String,          // "hostname-or-ip:port"
    pub wg_address: String,           // "10.0.0.2/24"
    pub wg_dns: String,
    pub wg_preshared_key: String,
    pub wg_keepalive: u32,            // default 25
    pub wg_route_all: bool,           // true = all traffic via tunnel, false = split tunnel

    // ---- WireGuard (server / home) ----
    pub wg_listen_port: u16,          // default 51820
    pub wg_server_private_key: String,
    pub wg_server_public_key: String, // derived, cached for display
    pub wg_peers: Vec<WgPeer>,        // travel boxes this home box serves
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgPeer {
    pub name: String,                 // friendly label, e.g. "My travel box"
    pub public_key: String,
    pub tunnel_ip: String,            // e.g. "10.0.0.2"
}
```

### Backward compatibility
- `Config` gets `#[serde(default)] pub vpn: VpnConfig`.
- `VpnConfig` and `WgPeer` implement `Default`; every field uses serde defaults.
- Old config files (no `vpn` key) parse to defaults → backend `none`, feature off. No migration.
- No `#[serde(deny_unknown_fields)]` anywhere (matches existing style).

### Validation — `VpnConfig::validate() -> Result<(), Vec<String>>`
Called from `Config::validate()` (config.rs:111). Plain-language errors:
- backend ∈ {none, tailscale, wireguard}, role ∈ {home, travel}
- tailscale: auth key present (or existing `tailscaled` state); exit node required for travel
- wireguard client: private key, peer public key, endpoint, address must be present; endpoint has a port; address is valid CIDR
- wireguard server: listen port in range; each peer has a valid pubkey + tunnel IP (unique)

## 4. Module layout — new `src/vpn/mod.rs`

All command execution via `std::process::Command` (matches codebase style — no new crates needed; keygen shells out to `wg genkey`/`wg pubkey`).

### 4.1 `status() -> VpnStatus`
Read-only, safe to call frequently (used by `/api/status` polling):
- `backend`, `role` (from config)
- `installed`: `tailscale` binary present, `wg`/`wg-quick` present, `tailscaled` service state
- Tailscale: `tailscale status --json` → state (`Running`/`NeedsLogin`/`Stopped`), hostname, self IP, exit node in use
- WireGuard: `wg show wg0` → handshake age, peer endpoint, transfer; `ip -brief addr show wg0`
- `kill_switch`: whether the block chain is active
- errors are surfaced, never panics

### 4.2 `apply(&Config) -> Result<(), String>`
Idempotent full (re)configuration. Called at boot and from the API after save.
Order:
1. `ensure_install()` — if binaries missing, log clear error (package install should have done it; `/api/vpn/install` is the repair path). Non-fatal unless backend enabled.
2. `teardown()` — remove VPN nft table, `wg-quick down wg0` (if up), `tailscale down` (only if we own the tailscale state).
3. Dispatch on `backend`:
   - `none` → nothing more.
   - `tailscale` → §6.
   - `wireguard` → §5.
4. Re-assert the VPN nft table (masquerade/forward/kill-switch) **after** any firewall ruleset re-apply (see §7).

### 4.3 `stop()` / `teardown()`
Used on backend switch and on reset (`/api/reset`).

### 4.4 `install()` (repair path)
- `apt-get install -y wireguard-tools` (best-effort, logs output).
- Tailscale: download the official `.deb` from `https://pkgs.tailscale.com/stable/<distro>/<arch>.tgz` and `dpkg -i`. **Does not enable or start tailscaled.**
- Returns log lines for the UI progress panel.

### 4.5 `gen_keys(scope) -> KeyPair`
`wg genkey` → private; `wg pubkey` from it → public. No new crypto dependency.

### 4.6 `import_conf(text: &str) -> Result<VpnConfig, String>`
Parse a standard WireGuard `.conf` (from any provider or `wg-quick`) and map exactly as esp32_nat_router does:

| `.conf` line | Maps to |
|---|---|
| `PrivateKey` | `wg_private_key` |
| `Address = 10.2.0.2/32` | `wg_address` (CIDR kept) |
| `DNS = 10.2.0.1` | `wg_dns` |
| `PublicKey` | `wg_peer_public_key` |
| `PresharedKey` | `wg_preshared_key` |
| `Endpoint = host:51820` | `wg_endpoint` |
| `PersistentKeepalive` | `wg_keepalive` |
| `AllowedIPs = 0.0.0.0/0` | `wg_route_all = true`; anything else → `false` |

Rules: IPv4 only (skip IPv6 `Address`/`DNS`); first `Address` used; single peer (last `[Peer]` wins); `PrivateKey`, `PublicKey`, `Endpoint`, `Address` required — otherwise reject and leave config unchanged. Returns the populated `VpnConfig` for the user to review before saving.

## 5. WireGuard implementation

### 5.1 Client (travel box)
Write `/etc/wireguard/wg0.conf`:

```ini
[Interface]
PrivateKey = <wg_private_key>
Address = <wg_address>
DNS = <wg_dns>                      # optional
ListenPort = 51820                  # source port (helps NAT stability)

[Peer]
PublicKey = <wg_peer_public_key>
PresharedKey = <wg_preshared_key>   # optional
Endpoint = <wg_endpoint>
PersistentKeepalive = <wg_keepalive>
AllowedIPs = 0.0.0.0/0              # route-all; or the tunnel subnet for split tunnel
```

- `wg-quick up wg0`.
- **MSS clamp**: `nft` rule `oifname "wg0" tcp flags syn tcp option maxseg size set rt mtu` + PMTU 1440 (esp32 behaviour: MSS 1380 / PMTU 1440).
- **Kill switch** (route-all mode): a chain that drops AP-client non-local traffic whenever `wg0` has no live handshake (checked by `status()`; rules re-asserted on state change). Configurable off.
- `wg-quick` creates the default route via `wg0` (with a host route for the endpoint), so the existing `detect_uplink()` (firewall/mod.rs:7) returns `wg0` automatically and the existing ruleset follows.

### 5.2 Server (home box)
Write `/etc/wireguard/wg0.conf`:

```ini
[Interface]
PrivateKey = <wg_server_private_key>
Address = 10.0.0.1/24
ListenPort = <wg_listen_port>
# IP forwarding + NAT for client traffic
PostUp = sysctl -w net.ipv4.ip_forward=1; nft ...  # applied by travel-net instead
PostDown = ...

[Peer]   # one per wg_peers
# <name>
PublicKey = <peer.public_key>
AllowedIPs = <peer.tunnel_ip>/32      # tunnel IP only — see note below
```

- `wg-quick up wg0`.
- **Open UDP**: nft table `travel-vpn` input accept `udp dport <listen_port>` (all interfaces).
- **NAT for wg clients**: masquerade `wg0` source subnet → home WAN (in addition to the existing AP→WAN rule).
- **Why tunnel-IP-only `AllowedIPs`?** The travel box masquerades its AP
  traffic out `wg0` (`oifname "wg0" masquerade`), so the home server sees it
  addressed to the peer's tunnel IP and only needs a route to that /32. Adding
  the travel box's AP subnet here instead caused a real failure: on a travel-net
  home box whose own AP uses the same default subnet (`192.168.4.0/24`),
  `wg-quick up` tried `ip route add 192.168.4.0/24 dev wg0`, hit the already
  existing route via `wlan1`, and rolled the interface back (verified on
  NanoPi, T13). The manual guide (`docs/VPN_GUIDE.md` 2B) matches: tunnel IP only.

### 5.3 Pairing (guided, both sides)
- **Travel box**: `POST /api/vpn/genkeys {scope:"client"}` → shows public key + "my tunnel IP" + a *copy-ready request block*.
- **Home box (travel-net)**: `POST /api/vpn/peers {action:"add", name, public_key, tunnel_ip}` → writes peer, `wg-quick down/up wg0`, then `GET` a *copy-ready client config* (all fields the travel box needs).
- **Home box (existing Linux machine — non-travel-net)**: the web console and `docs/VPN_GUIDE.md` show the exact manual steps (install, keys, `wg0.conf`, firewall, `systemctl enable --now wg-quick@wg0`) so the user can set up the same server on any Debian/Ubuntu/Raspberry Pi machine and paste the resulting details into the travel box. This mirrors the esp32_nat_router guide.

## 6. Tailscale implementation

- Package install puts `tailscaled` on disk but **disabled** (`systemctl disable tailscaled`, no start). travel-net starts/enables it only when the user applies a Tailscale backend.
- **Home role**: `tailscale up --authkey=<ts_auth_key> --hostname=<ts_hostname> --advertise-exit-node --advertise-routes=<ts_advertise_routes>`
  - `--advertise-routes` lets travel devices reach the home LAN.
  - Exit node + routes must be **approved once** in the Tailscale admin console (documented in the guide + help text).
- **Travel role**: `tailscale up --authkey=<ts_auth_key> --hostname=<ts_hostname> --exit-node=<ts_exit_node> --exit-node-allow-lan-access=<ts_allow_lan> --accept-routes`
  - `--accept-routes` pulls the home LAN routes; `--exit-node-allow-lan-access` keeps the hotel/4G LAN reachable while tunnelled.
- **Backend=none**: `tailscale down` (state kept so re-up is fast).
- **Read-only rootfs (A5E)**: `/var/lib/tailscale` is tmpfs → state lost on reboot. travel-net re-runs `tailscale up` with the saved `ts_auth_key` at boot (auth key must be non-ephemeral) so the box rejoins automatically.
- Status from `tailscale status --json`; connectivity via `tailscale netcheck` (best-effort, for the UI).

## 7. nftables integration — dedicated `travel-vpn` table

Owned and re-asserted by `vpn/mod.rs`. Independent of `travel-net.nft`'s `flush ruleset` (firewall/mod.rs:120), so it:

- survives any firewall ruleset re-apply (hostapd backend),
- works on NM boxes where the firewall module is never invoked (main.rs:61),
- does not wipe the client-block set (`travel-net-block`, clients.rs) — separate table.

```text
table inet travel-vpn {
    chain postrouting {
        type nat hook postrouting priority srcnat;
        oifname "wg0"  masquerade     # AP subnet → tunnel   (also tailscale0)
        oifname "wg0"  ip saddr 10.0.0.0/24 masquerade       # home server: wg clients → WAN
    }
    chain forward { ... iifname <ap_iface> oifname wg0/tailscale0 accept ... return path ... }
    chain input { ... udp dport <listen_port> accept ... }
    chain killswitch { ... drop non-local unless tunnel handshake live ... }
}
```

Ordering rule: `vpn::apply` re-asserts this table **after** `firewall::apply_ruleset` in both the boot path and the API background task.

## 8. API — `src/web/api.rs`

All endpoints auth-gated with `is_authed` (api.rs:21) except none (existing pattern; `/api/vpn` is config+status so it stays authed like `/api/config`).

| Endpoint | Method | Purpose |
|---|---|---|
| `/api/vpn` | GET | `{ config: VpnConfig, status: VpnStatus }` |
| `/api/vpn` | POST | body = `VpnConfig` JSON → validate → save `/etc/travel-net/config.json` → spawn `vpn::apply` in background (same pattern as api.rs:264–268) |
| `/api/vpn/import` | POST | `{ conf }` → `import_conf()` → returns parsed `VpnConfig` for review |
| `/api/vpn/genkeys` | POST | `{ scope: "client"\|"server" }` → returns keypair + ready-to-paste config block |
| `/api/vpn/peers` | POST | `{ action: add\|remove, name, public_key, tunnel_ip }` (home server) |
| `/api/vpn/install` | POST | repair/install missing packages, returns log lines |

**Status surface**: extend `StatusResponse` (api.rs:31–45) with a `vpn: VpnStatus` object so the dashboard shows tunnel state; `api_status` (api.rs:133) populates it (cheap, cached ~10s).

## 9. Web UI — `/vpn` page

- New `templates/vpn.html` (style: `templates/config.html` `buildForm`/`saveConfig` + `templates/clients.html` structure). Registered as `VPN_HTML` in `src/templates.rs`, route + auth handler in `src/web/pages.rs` (pattern pages.rs:27–30).
- `templates/index.html`: add **VPN** nav button (button-container, ~line 49) and a VPN status row on the dashboard.
- Page structure:
  1. **What this does** panel (plain language, one paragraph).
  2. **Role** toggle: Home box / Travel box.
  3. **Backend** selector: None / Tailscale / WireGuard — fields swap dynamically (pattern: `onBandChange`, config.html:152).
  4. Per-field `.hint` help text — see §10 for exact wording (non-technical).
  5. **WireGuard helpers**: Import .conf textarea + button; keygen button; home-mode peer table (add/remove; "show client config"); link to the existing-home-machine instructions.
  6. **Status panel**: installed packages, tunnel state, handshake/exit node, kill-switch indicator; 10 s poll.
  7. **Save & Apply** (`ok-button`), inline success/error message (`.message`).

## 10. Help-text spec (plain language for non-technical users)

Every field carries a `.hint`. Exact wording is finalized in task T9 but the required content per field:

| Field | Help text (plain language) |
|---|---|
| Role | "Pick Home box if this device stays at home, always on. Pick Travel box if this is the one you carry." |
| Backend | "None turns the VPN off. Tailscale is the easiest — no router changes needed. WireGuard is fully under your control but needs one port opened on your home router." |
| ts_auth_key | "A secret key from your Tailscale account (Admin Console → Settings → Keys → Generate). Paste it here." |
| ts_hostname | "A name for this box on your Tailscale network, e.g. travel-box." |
| ts_exit_node | "The name of your home machine as it appears in Tailscale (e.g. home-pi). This is where your traffic exits the internet." |
| ts_advertise_routes | "Your home network range, usually 192.168.1.0/24 or whatever your home router shows. Lets you reach home devices (NAS, printers)." |
| ts_allow_lan | "Keep your current hotel/café Wi-Fi reachable while tunnelled. Leave on." |
| wg_private_key | "Your secret key. Leave empty and press 'Generate my key' — the box makes one for you." |
| wg_peer_public_key | "The public key of the home side. Copy it from the home box's screen (or from the instructions if your home machine is not a travel-net box)." |
| wg_endpoint | "Your home address + port, e.g. myhome.ddns.net:51820 or 203.0.113.10:51820." |
| wg_address | "This box's address inside the tunnel, e.g. 10.0.0.2/24. The home side gives you this." |
| wg_dns | "Optional. Leave empty to use your normal DNS." |
| wg_preshared_key | "Optional extra secret both sides share. Leave empty if unsure." |
| wg_keepalive | "Keep the tunnel alive through NAT. 25 is fine for almost everyone." |
| wg_route_all | "On = all internet traffic goes through home. Off = only the tunnel network, internet stays direct (split tunnel)." |
| wg_listen_port | "The door on your home box where travel boxes connect. 51820 is the standard." |
| Kill switch | "On = if the tunnel drops, travel devices lose internet instead of leaking your real location. Recommended." |

## 11. Boot wiring — `src/main.rs`

After AP/dnsmasq/firewall setup (main.rs:57–64):

```rust
if cfg.vpn.backend != "none" {
    crate::vpn::apply(&cfg).await;
}
```

No static `After=tailscaled.service` in the unit (tailscaled may not exist pre-install). The VPN module polls the `tailscaled` socket at runtime before issuing `tailscale up`.

## 12. Packaging

- **`debian/control` + generated per-arch Depends** (currently armhf=`hostapd,dnsmasq,wpasupplicant,nftables,iw`; arm64/amd64=`network-manager,wpasupplicant,iw`): add **`wireguard-tools`** to all three.
- **Tailscale**: added to **postinst** — download official `.deb` (`https://pkgs.tailscale.com/stable/<distro>/<arch>.tgz`, e.g. `bookworm`) and `dpkg -i` it, then **`systemctl disable tailscaled`** (installed, not enabled — per decision #1). postinst stays idempotent (`|| true` guards, already the style).
- `debian/postinst` and `pkg_deb/DEBIAN/postinst` both updated; reconcile with the untracked `pkg_deb/` staging (its `DEBIAN/control` is currently empty and filled at publish time).
- **Read-only rootfs**: documented (AGENTS.md pattern) — `/var/lib/tailscale` tmpfs on A5E handled by boot re-auth (§6); WireGuard keys live in `/etc/travel-net/config.json` (already the config model).

## 13. Build & test plan

1. `cargo build --release` (host, catches compile errors) + `cargo-zigbuild` for `aarch64-unknown-linux-gnu`, `armv7-unknown-linux-gnueabihf`, `x86_64-unknown-linux-gnu`.
2. Deploy + test on **NanoPi NEO Air** (192.168.200.71): travel role end-to-end — WireGuard via .conf import and via pairing; Tailscale travel (exit node to a home node); kill-switch on/off; home-role server (keygen, peer add, client-conf generation, real handshake via a second wg interface).
3. Home role: on a travel-net box with a real uplink; and validate the *existing-home-machine* WireGuard instructions against a stock Debian/Raspberry Pi.
4. Kill-switch test: stop tunnel mid-session, confirm travel clients lose internet; restart, confirm recovery.
5. Rebuild + republish the three `.deb`s to gh-pages apt repo (per-arch Depends updated), then `apt update && apt upgrade travel-net` on RPi2.
6. Full web-console walkthrough as a non-technical user (guide-following test).

## 14. Task tracking

See [`TASKS.json`](../TASKS.json) — tasks are executed sequentially; each is marked `todo` → `inprogress` → `completed` as work lands.
