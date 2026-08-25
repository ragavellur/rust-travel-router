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

## Rules Summary
1. NEVER touch /var/lib/dpkg/status
2. NEVER modify/remove NM config or override (postinst OR prerm)
3. NEVER run apt without clock verification (date +%Y >= 2024)
4. NEVER run apt without dpkg --configure -a first
5. NEVER run apt from systemd service without Stdio::null() on stdin
6. NEVER pin tailscale version (1.102.3 works on all tested kernels)
7. ALWAYS warn user to connect via non-AP interface before uninstall
8. ALWAYS run apt-get update before apt-get install in postinst
