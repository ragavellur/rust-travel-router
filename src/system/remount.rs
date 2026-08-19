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

/// Unmount ALL overlay bind-mounts so rootfs can be remounted.
/// This is the ONLY reliable way to make `mount -o remount,ro /` succeed.
/// The overlay service will re-create them at next boot.
fn unmount_all_overlays() {
    // resolv.conf has its own special bind-mount
    if is_overlay_active("/etc/resolv.conf") {
        let _ = Command::new("umount").arg("/etc/resolv.conf").output();
    }
    for dir in OVERLAY_DIRS {
        if is_overlay_active(dir) {
            let _ = Command::new("umount").arg(dir).output();
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
                line.replace("ro,", "").replace(",ro,", ",").replace(",ro ", " ").replace(" ro,", " ,")
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
        return write_result;
    }

    Ok(())
}

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

    remount_rw()?;
    let write_result = std::fs::write("/etc/fstab", &new_fstab)
        .map_err(|e| format!("Write fstab: {e}"));
    if let Err(e) = write_result {
        let _ = remount_ro();
        return Err(e);
    }

    remount_ro()?;

    let _ = Command::new("systemctl")
        .args(["enable", "travel-net-overlay.service"])
        .output();

    persist_config();
    persist_nm_connections();

    Ok(())
}

fn persist_dir(src: &str) -> Result<(), String> {
    let tmpdir = tmpdir_for(src);

    if !is_overlay_active(src) {
        return Ok(());
    }

    let unbind = Command::new("umount").arg(src).output();
    if !unbind.map(|o| o.status.success()).unwrap_or(false) {
        return Err(format!("Failed to unbind {src}"));
    }

    remount_rw()?;

    let cp = Command::new("cp")
        .args(["-a", &format!("{tmpdir}/."), src])
        .output()
        .map_err(|e| format!("cp: {e}"))?;
    if !cp.status.success() {
        let err = String::from_utf8_lossy(&cp.stderr);
        tracing::warn!("persist cp error for {src}: {err}");
    }

    let _ = remount_ro();

    let _ = Command::new("mount")
        .args(["--bind", &tmpdir, src])
        .output();

    tracing::info!("Persisted {src} to disk");
    Ok(())
}

pub fn persist_config() {
    if let Err(e) = persist_dir("/etc/travel-net") {
        tracing::warn!("Failed to persist config: {e}");
    }
}

pub fn persist_nm_connections() {
    if let Err(e) = persist_dir("/etc/NetworkManager/system-connections") {
        tracing::warn!("Failed to persist NM connections: {e}");
    }
}

#[allow(dead_code)]
pub fn persist_all() {
    for dir in OVERLAY_DIRS {
        if let Err(e) = persist_dir(dir) {
            tracing::warn!("Failed to persist {dir}: {e}");
        }
    }
}
