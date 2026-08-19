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
        "/run/overlay-{}",
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

/// Lazy-unmount ALL overlay bind-mounts so rootfs can be remounted rw.
/// `umount -l` detaches immediately even if processes hold files open.
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
/// This ALWAYS succeeds — changing ro→rw doesn't require closing FDs.
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

// ── Public API ───────────────────────────────────────────────────────────

pub fn safe_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    if is_file_on_overlay(path) {
        // Fast path: write to tmpfs directly
        return std::fs::write(path, content).map_err(|e| format!("Write {}: {e}", path.display()));
    }
    // Non-overlay file: remount rw, write, done. No remount ro.
    remount_rw()?;
    std::fs::write(path, content).map_err(|e| format!("Write {}: {e}", path.display()))
}

pub fn safe_create_dir_all(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if is_file_on_overlay(path) {
        return std::fs::create_dir_all(path).map_err(|e| format!("Mkdir {}: {e}", path.display()));
    }
    remount_rw()?;
    std::fs::create_dir_all(path).map_err(|e| format!("Mkdir {}: {e}", path.display()))
}

pub fn safe_set_permissions(path: &std::path::Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if is_file_on_overlay(path) {
        return std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("Chmod {}: {e}", path.display()));
    }
    remount_rw()?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("Chmod {}: {e}", path.display()))
}

pub fn save_config(path: &std::path::Path, cfg: &crate::config::Config) -> Result<(), Box<dyn std::error::Error>> {
    let dir = path.parent().unwrap_or(std::path::Path::new("/"));
    let dir_str = dir.to_str().unwrap_or("");

    if is_overlay_active(dir_str) {
        // Fast path: write to tmpfs, persist to ext4
        let tmp = path.with_extension("json.tmp");
        let data = serde_json::to_string_pretty(cfg)?;
        std::fs::write(&tmp, &data)?;
        std::fs::rename(&tmp, path)?;
        persist_config();
        return Ok(());
    }

    // Non-overlay: remount rw, write, persist
    remount_rw()?;
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, path)?;
    persist_config();
    Ok(())
}

/// Write fstab with ro and enable overlay service.
/// Does NOT remount — reboot is required for RO to take effect.
pub fn make_readonly() -> Result<(), String> {
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

    // Write fstab — if rootfs is rw, write directly. If ro, remount rw first.
    if is_rootfs_readonly() {
        remount_rw()?;
    }
    std::fs::write("/etc/fstab", &new_fstab)
        .map_err(|e| format!("Write fstab: {e}"))?;

    // Enable overlay service for next boot
    let _ = Command::new("systemctl")
        .args(["enable", "travel-net-overlay.service"])
        .output();

    // Persist current config to ext4 so it survives reboot
    persist_config();
    persist_nm_connections();

    // Sync
    let _ = Command::new("sync").output();

    Ok(())
}

/// Write fstab without ro and disable overlay service.
/// Does NOT remount — reboot is required for RW to take effect.
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

    if is_rootfs_readonly() {
        remount_rw()?;
    }
    std::fs::write("/etc/fstab", &new_fstab)
        .map_err(|e| format!("Write fstab: {e}"))?;

    let _ = Command::new("systemctl")
        .args(["disable", "travel-net-overlay.service"])
        .output();

    let _ = Command::new("sync").output();

    Ok(())
}

// ── Overlay persistence (unbind → copy → rebind) ─────────────────────────

pub fn persist_config() {
    persist_overlay_dir("/etc/travel-net");
}

pub fn persist_nm_connections() {
    persist_overlay_dir("/etc/NetworkManager/system-connections");
}

fn persist_overlay_dir(dir: &str) {
    if !is_overlay_active(dir) {
        return;
    }
    let tmpdir = tmpdir_for(dir);

    // Unbind overlay — exposes ext4 dir
    let unbind = Command::new("umount").arg(dir).output();
    if !unbind.map(|o| o.status.success()).unwrap_or(false) {
        tracing::warn!("Failed to unbind {dir} for persist");
        return;
    }

    // Remount rw if needed (rootfs might be ro)
    let _ = remount_rw();

    // Copy tmpdir → ext4
    let cp = Command::new("cp")
        .args(["-a", &format!("{tmpdir}/."), dir])
        .output();
    if let Ok(o) = cp {
        if !o.status.success() {
            tracing::warn!("persist cp error for {dir}: {}", String::from_utf8_lossy(&o.stderr));
        }
    }

    // Rebind overlay
    let _ = Command::new("mount")
        .args(["--bind", &tmpdir, dir])
        .output();

    tracing::info!("Persisted {dir} to disk");
}

#[allow(dead_code)]
pub fn persist_all() {
    for dir in OVERLAY_DIRS {
        persist_overlay_dir(dir);
    }
}
