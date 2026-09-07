# ISSUE TRACKER — DO NOT REPEAT THESE

Every fix must be checked against this list before shipping.
If an issue is here, it was fixed before and MUST NOT be reintroduced.

## Critical Issues (device-breaking)

### 1. Wiping /var/lib/dpkg/status
- **Symptom**: Entire package database destroyed, NM/systemd everything breaks
- **Cause**: Manual rm of dpkg status during troubleshooting
- **Fix**: NEVER touch /var/lib/dpkg/status. Period.
- **Status**: Rule 1 in AGENTS.md

### 2. NM override removal breaks ethernet/networking
- **Symptom**: Device goes offline, no network after reboot
- **Cause**: prerm script removed /etc/systemd/system/NetworkManager.service.d/override.conf
- **Fix**: prerm NEVER touches NM config. Only stops/disables travel-net services.
- **Status**: Fixed in v0.2.31. Rule 2 in AGENTS.md expanded.

### 3. Running apt with wrong/stale clock corrupts apt lists
- **Symptom**: GPG signature failures, dpkg errors, "not installable"
- **Cause**: apt-get update on device without RTC, clock resets to 1970
- **Fix**: ALL scripts check `date +%Y >= 2024` before any apt operation
- **Status**: Rule 3 in AGENTS.md. postinst, tailscale service, vpn::install() all check.

### 4. dpkg --configure -a not run before apt-get install
- **Symptom**: "Sub-process /usr/bin/dpkg returned an error code (1)"
- **Cause**: Half-installed packages from earlier corruption block new installs
- **Fix**: Always run `dpkg --configure -a` and `apt-get -f install -y` before any apt install
- **Status**: Fixed in v0.2.29 (postinst + tailscale service), v0.2.30 (vpn::install in Rust)

### 5. debconf ioctl error when running apt from systemd service
- **Symptom**: "E: Cannot get debconf version. debconf: apt-extracttemplates failed: Inappropriate ioctl for device"
- **Cause**: apt-get runs without a TTY (from systemd service), debconf tries to read stdin
- **Fix**: All Command::new calls use `.stdin(Stdio::null())` via the `run()` helper
- **Status**: Fixed in v0.2.30

### 6. Tailscale version pin (1.76.*) causes downgrade failure
- **Symptom**: "tailscale install failed: E: Packages were downgraded and -y was used without --allow-downgrades"
- **Cause**: Code pinned `tailscale=1.76.*` but 1.102.3 already installed; apt refuses downgrade with -y
- **Fix**: Removed version pin entirely. 1.102.3 works fine on kernel 5.15.
- **Status**: Fixed in v0.2.32

### 7. Uninstall drops SSH when connected via Travel-Net AP
- **Symptom**: SSH session dies mid-uninstall, device appears frozen
- **Cause**: `dpkg -r travel-net` stops the AP service which kills the WiFi you're connected through
- **Fix**: Connect via ethernet/uplink WiFi before uninstall. NOT via Travel-Net AP.
- **Status**: Documented in README

## Package Dependency Issues

### 8. Fresh Radxa images have empty apt lists
- **Symptom**: dpkg -i / apt-get install ./foo.deb fails with "not installable" for deps
- **Cause**: Radxa Debian images ship with empty /var/lib/apt/lists/
- **Fix**: postinst runs `apt-get update` before installing deps. Depends field empty (Recommends only).
- **Status**: Fixed in v0.2.27

### 9. nftables not installed after fresh install
- **Symptom**: "nft: No such file or directory (os error 2)" in web UI
- **Cause**: postinst's `apt-get install nftables` failed silently (stale apt lists from earlier corruption) due to `|| true`
- **Fix**: Ensure apt lists are fresh before postinst runs. User must run `apt-get update` if lists are stale.
- **Status**: Partially fixed. postinst runs apt-get update but `|| true` swallows failures. Consider retrying.

## Build/Deploy Issues

### 10. Wrong binary architecture in .deb
- **Symptom**: "Exec format error" on device
- **Cause**: build-debs.sh must generate per-arch control files with correct Architecture field
- **Status**: build-debs.sh generates correct per-arch control. Verified by `dpkg-deb -I`.

### 11. Corrupted dpkg status causes binary corruption on install
- **Symptom**: "Exec format error" after dpkg -i .deb — binary at /usr/sbin/travel-net is garbage data, not ELF
- **Cause**: Previous dpkg status corruption makes dpkg unable to track files properly; subsequent installs write corrupted data
- **Fix**: Clear dpkg status (`cp /dev/null /var/lib/dpkg/status`), rebuild with `dpkg --configure -a`, then manually copy binary via scp
- **Prevention**: The ONLY real fix is a clean SD card flash. Remote fixes are temporary.

## Rules Summary
1. NEVER touch /var/lib/dpkg/status
2. NEVER modify/remove NM config or override (postinst OR prerm)
3. NEVER run apt without clock verification (date +%Y >= 2024)
4. NEVER run apt without dpkg --configure -a first
5. NEVER run apt from systemd service without Stdio::null() on stdin
6. NEVER pin tailscale version (1.102.3 works on all tested kernels)
7. ALWAYS warn user to connect via non-AP interface before uninstall
8. ALWAYS run apt-get update before apt-get install in postinst

### 12. Single-radio brcmfmac: AP channel selection & HT capabilities
- **Symptom**: hostapd AP fails or behaves wrong on NanoPi NEO Air (brcmfmac BCM43430)
- **Cause 1**: `ht_capab` containing `[LDPC]` → hostapd logs `Driver does not support configured HT capability [LDPC]` → `Unable to setup interface` → exit 256, even on a valid channel. This was the REAL reason channels 1/6/11 "failed" while STA was on 3 — every earlier failure was LDPC rejection, not channel mismatch.
- **Cause 2**: Parsing `iw dev wlanX link` / `iw scan` output: `freq: 2422.0` and `signal: -52.00 dBm` are FLOATS. `parse::<u32>()` fails → `detect_sta_channel()` always returned None and `scan_least_congested_channel()` saw zero networks, picking channel 1 blindly.
- **Cause 3**: hostapd on a channel != STA channel can "start" but the firmware silently keeps the radio on the STA channel. Beacons advertise the wrong channel → AP invisible/broken for clients. The monitor must verify the ACTUAL channel via `iw dev wlan1 info`, not assume config was honored.
- **Cause 4**: After a failed hostapd start the interface is stuck DISABLED; reusing it fails forever. Must `iw dev wlan1 del` + recreate before every attempt.
- **Fix**: HT20-only 2.4GHz `ht_capab=[HT20][SHORT-GI-20]` (no LDPC, no HT40). Parse freqs/signals as f32. Detect STA channel first (with retries) and force AP to it; only scan when no STA. Monitor compares actual AP channel to STA channel and restarts on mismatch; recreates the interface on every restart.
- **Status**: Fixed in v0.2.33. Verified on NanoPi: AP on STA channel 3, survives STA drop, self-heals killed hostapd, web UI 200.
