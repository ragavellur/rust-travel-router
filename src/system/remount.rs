use std::process::Command;

const OVERLAY_DIRS: &[&str] = &[
    "/etc/travel-net",
    "/etc/NetworkManager/system-connections",
    "/etc/NetworkManager/dnsmasq-shared.d",
    "/etc/NetworkManager/conf.d",
    "/etc/hostapd",
    "/etc/wpa_supplicant",
    "/etc/dnsmasq.d",
    "/etc/wireguard",
    "/etc/modprobe.d",
    "/var/lib/dpkg",
    "/var/cache/apt",
    "/var/lib/apt",
    "/var/lib/NetworkManager",
    "/var/lib/systemd",
    "/var/log",
    "/var/tmp",
    "/etc/apt/sources.list.d",
    "/usr/share/keyrings",
];

fn tmpdir_for(path: &str) -> String {
    format!(
        "/tmp/overlay-{}",
        path.replace('/', "_").trim_start_matches('_')
    )
}

fn read_root_mount_opts() -> String {
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[1] == "/" {
                return parts[3].to_string();
            }
        }
    }
    String::new()
}

pub fn is_rootfs_readonly() -> bool {
    let opts = read_root_mount_opts();
    opts.contains(",ro") || opts.starts_with("ro,")
}

pub fn is_overlay_active(dir: &str) -> bool {
    Command::new("mountpoint")
        .args(["-q", dir])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Lazy-unmount ALL overlay bind-mounts so rootfs can be remounted.
/// `umount -l` detaches the mount immediately even if processes hold files open.
/// The kernel cleans up references in the background.
/// The overlay service will re-create them at next boot.
fn unmount_all_overlays() {
    if is_overlay_active("/etc/resolv.conf") {
        let _ = Command::new("umount").args(["-l", "/etc/resolv.conf"]).output();
    }
    for dir in OVERLAY_DIRS {
        if is_overlay_active(dir) {
            let _ = Command::new("umount").args(["-l", dir]).output();
        }
    }
}

pub fn is_file_on_overlay(path: &std::path::Path) -> bool {
    let mut current = path.parent().unwrap_or(std::path::Path::new("/"));
    loop {
        let s = current.to_str().unwrap_or("");
        if is_overlay_active(s) {
            return true;
        }
        if current == std::path::Path::new("/") {
            break;
        }
        current = current.parent().unwrap_or(std::path::Path::new("/"));
    }
    false
}

/// Unmount all overlays, then remount rootfs rw.
fn remount_rw() -> Result<(), String> {
    unmount_all_overlays();
    let output = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output()
        .map_err(|e| format!("mount remount,rw: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("mount remount,rw / failed: {stderr}"));
    }
    Ok(())
}

/// Unmount all overlays, sync, then remount rootfs ro.
fn remount_ro() -> Result<(), String> {
    unmount_all_overlays();
    let _ = Command::new("sync").output();
    let output = Command::new("mount")
        .args(["-o", "remount,ro", "/"])
        .output()
        .map_err(|e| format!("mount remount,ro: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("mount remount,ro / failed: {stderr}"));
    }
    Ok(())
}

/// Copy tmpdir content to the real ext4 directory (persist overlay to disk).
/// Called while rootfs is rw and overlays are unmounted.
fn persist_dir_to_disk(dir: &str) {
    let tmpdir = tmpdir_for(dir);
    if !std::path::Path::new(&tmpdir).exists() {
        return;
    }
    // Ensure the ext4 target dir exists
    let _ = std::fs::create_dir_all(dir);
    // Copy tmpdir → ext4
    if let Err(e) = std::fs::read_dir(&tmpdir) {
        tracing::warn!("Cannot read tmpdir {tmpdir}: {e}");
        return;
    }
    let cp = Command::new("cp")
        .args(["-a", &format!("{tmpdir}/."), dir])
        .output();
    match cp {
        Ok(o) if !o.status.success() => {
            let err = String::from_utf8_lossy(&o.stderr);
            tracing::warn!("persist cp error for {dir}: {err}");
        }
        Err(e) => tracing::warn!("persist cp spawn error for {dir}: {e}"),
        _ => {}
    }
}

/// Pre-create ALL overlay tmpdirs in /tmp so the overlay script finds them
/// at boot when rootfs is ro and mkdir would fail.
fn precreate_tmpdirs() {
    for dir in OVERLAY_DIRS {
        let tmpdir = tmpdir_for(dir);
        let _ = std::fs::create_dir_all(&tmpdir);
    }
    let _ = std::fs::create_dir_all("/tmp/overlay-etc_resolv_conf");
}

pub fn safe_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    if is_file_on_overlay(path) {
        std::fs::write(path, content).map_err(|e| format!("Write {}: {e}", path.display()))?;
        return Ok(());
    }
    remount_rw()?;
    let result = std::fs::write(path, content)
        .map_err(|e| format!("Write {}: {e}", path.display()));
    let _ = remount_ro();
    result
}

pub fn safe_write_bytes(path: &std::path::Path, content: &[u8]) -> Result<(), String> {
    if is_file_on_overlay(path) {
        std::fs::write(path, content).map_err(|e| format!("Write {}: {e}", path.display()))?;
        return Ok(());
    }
    remount_rw()?;
    let result = std::fs::write(path, content)
        .map_err(|e| format!("Write {}: {e}", path.display()));
    let _ = remount_ro();
    result
}

pub fn safe_create_dir_all(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if is_file_on_overlay(path) {
        std::fs::create_dir_all(path).map_err(|e| format!("Mkdir {}: {e}", path.display()))?;
        return Ok(());
    }
    remount_rw()?;
    let result = std::fs::create_dir_all(path)
        .map_err(|e| format!("Mkdir {}: {e}", path.display()));
    let _ = remount_ro();
    result
}

pub fn safe_set_permissions(path: &std::path::Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if is_file_on_overlay(path) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("Chmod {}: {e}", path.display()))?;
        return Ok(());
    }
    remount_rw()?;
    let result = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("Chmod {}: {e}", path.display()));
    let _ = remount_ro();
    result
}

pub fn save_config(path: &std::path::Path, cfg: &crate::config::Config) -> Result<(), Box<dyn std::error::Error>> {
    let dir = path.parent().unwrap_or(std::path::Path::new("/"));
    let dir_str = dir.to_str().unwrap_or("");

    if is_overlay_active(dir_str) {
        let tmp = path.with_extension("json.tmp");
        let data = serde_json::to_string_pretty(cfg)?;
        std::fs::write(&tmp, &data)?;
        std::fs::rename(&tmp, path)?;
        persist_config();
        return Ok(());
    }

    remount_rw()?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let tmp = path.with_extension("json.tmp");
        let data = serde_json::to_string_pretty(cfg)?;
        std::fs::write(&tmp, &data)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();

    if result.is_ok() {
        persist_config();
    }
    let _ = remount_ro();
    result
}

pub fn make_writable() -> Result<(), String> {
    let fstab = std::fs::read_to_string("/etc/fstab").map_err(|e| format!("Read fstab: {e}"))?;
    let new_fstab = fstab
        .lines()
        .map(|line| {
            if (line.starts_with("UUID=") || line.starts_with("/dev/"))
                && line.contains(" / ext4 ")
            {
                line.replace("ro,", "")
                    .replace(",ro,", ",")
                    .replace(",ro ", " ")
                    .replace(" ro,", " ,")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    remount_rw()?;
    let write_result = std::fs::write("/etc/fstab", &new_fstab)
        .map_err(|e| format!("Write fstab: {e}"));
    if write_result.is_err() {
        let _ = remount_ro();
    }
    write_result
}

/// Lock rootfs to read-only. Flow:
/// 1. remount_rw (unmounts all overlays, remounts rootfs rw)
/// 2. Write fstab with ro
/// 3. Persist ALL overlay content from tmpdirs to ext4
/// 4. Pre-create ALL tmpdirs on ext4 for overlay script at boot
/// 5. remount_ro
/// 6. Enable overlay service for next boot
pub fn make_readonly() -> Result<(), String> {
    let _ = Command::new("sync").output();

    let fstab = std::fs::read_to_string("/etc/fstab").map_err(|e| format!("Read fstab: {e}"))?;
    let new_fstab = fstab
        .lines()
        .map(|line| {
            if (line.starts_with("UUID=") || line.starts_with("/dev/"))
                && line.contains(" / ext4 ")
                && !line.contains("ro")
            {
                line.replacen("defaults", "ro,defaults", 1)
                    .replacen("errors=remount-ro", "ro,errors=remount-ro", 1)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Step 1: Unmount all overlays, remount rootfs rw
    remount_rw()?;

    // Step 2: Write fstab
    std::fs::write("/etc/fstab", &new_fstab)
        .map_err(|e| {
            let _ = remount_ro();
            format!("Write fstab: {e}")
        })?;

    // Step 3: Persist ALL overlay content to ext4 (tmpdir → real dirs)
    for dir in OVERLAY_DIRS {
        persist_dir_to_disk(dir);
    }
    // Also persist resolv.conf if it was in tmpfs
    let resolv_tmpdir = "/tmp/overlay-etc_resolv_conf";
    if std::path::Path::new(resolv_tmpdir).join("resolv.conf").exists() {
        let _ = std::fs::copy(
            format!("{resolv_tmpdir}/resolv.conf"),
            "/etc/resolv.conf",
        );
    }

    // Step 4: Pre-create ALL tmpdirs on ext4 so overlay script finds them at boot
    precreate_tmpdirs();

    // Step 5: Sync + remount ro
    let _ = Command::new("sync").output();
    remount_ro()?;

    // Step 6: Enable overlay service for next boot
    let _ = Command::new("systemctl")
        .args(["enable", "travel-net-overlay.service"])
        .output();

    Ok(())
}

pub fn persist_config() {
    if !is_overlay_active("/etc/travel-net") {
        return;
    }
    let tmpdir = tmpdir_for("/etc/travel-net");
    let unbind = Command::new("umount").arg("/etc/travel-net").output();
    if !unbind.map(|o| o.status.success()).unwrap_or(false) {
        tracing::warn!("Failed to unbind /etc/travel-net for persist");
        return;
    }
    let cp = Command::new("cp")
        .args(["-a", &format!("{tmpdir}/."), "/etc/travel-net"])
        .output();
    if let Ok(o) = cp {
        if !o.status.success() {
            tracing::warn!("persist cp error: {}", String::from_utf8_lossy(&o.stderr));
        }
    }
    let _ = Command::new("mount")
        .args(["--bind", &tmpdir, "/etc/travel-net"])
        .output();
    tracing::info!("Persisted /etc/travel-net to disk");
}

pub fn persist_nm_connections() {
    if !is_overlay_active("/etc/NetworkManager/system-connections") {
        return;
    }
    let dir = "/etc/NetworkManager/system-connections";
    let tmpdir = tmpdir_for(dir);
    let unbind = Command::new("umount").arg(dir).output();
    if !unbind.map(|o| o.status.success()).unwrap_or(false) {
        tracing::warn!("Failed to unbind {dir} for persist");
        return;
    }
    let cp = Command::new("cp")
        .args(["-a", &format!("{tmpdir}/."), dir])
        .output();
    if let Ok(o) = cp {
        if !o.status.success() {
            tracing::warn!("persist cp error: {}", String::from_utf8_lossy(&o.stderr));
        }
    }
    let _ = Command::new("mount")
        .args(["--bind", &tmpdir, dir])
        .output();
    tracing::info!("Persisted {dir} to disk");
}

#[allow(dead_code)]
pub fn persist_all() {
    for dir in OVERLAY_DIRS {
        let tmpdir = tmpdir_for(dir);
        if !is_overlay_active(dir) {
            continue;
        }
        let unbind = Command::new("umount").arg(dir).output();
        if !unbind.map(|o| o.status.success()).unwrap_or(false) {
            tracing::warn!("Failed to unbind {dir}");
            continue;
        }
        let cp = Command::new("cp")
            .args(["-a", &format!("{tmpdir}/."), dir])
            .output();
        if let Ok(o) = cp {
            if !o.status.success() {
                tracing::warn!("persist cp error for {dir}: {}", String::from_utf8_lossy(&o.stderr));
            }
        }
        let _ = Command::new("mount")
            .args(["--bind", &tmpdir, dir])
            .output();
        tracing::info!("Persisted {dir} to disk");
    }
}
