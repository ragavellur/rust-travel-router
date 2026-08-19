use std::process::Command;

// ── Rootfs state detection ──────────────────────────────────────────────

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

// ── Remount helpers ─────────────────────────────────────────────────────

fn remount_rw() -> Result<(), String> {
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

/// If rootfs is ro, remount rw. Returns whether we did a remount.
fn ensure_rw() -> Result<bool, String> {
    if is_rootfs_readonly() {
        remount_rw()?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn finish_write(did_remount: bool) {
    if did_remount {
        let _ = remount_ro();
    }
}

// ── Public API ──────────────────────────────────────────────────────────

pub fn safe_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    let did = ensure_rw()?;
    let result = std::fs::write(path, content)
        .map_err(|e| format!("Write {}: {e}", path.display()));
    if did { let _ = Command::new("sync").output(); }
    finish_write(did);
    result
}

pub fn safe_create_dir_all(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let did = ensure_rw()?;
    let result = std::fs::create_dir_all(path)
        .map_err(|e| format!("Mkdir {}: {e}", path.display()));
    if did { let _ = Command::new("sync").output(); }
    finish_write(did);
    result
}

pub fn safe_set_permissions(path: &std::path::Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let did = ensure_rw()?;
    let result = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("Chmod {}: {e}", path.display()));
    if did { let _ = Command::new("sync").output(); }
    finish_write(did);
    result
}

pub fn save_config(path: &std::path::Path, cfg: &crate::config::Config) -> Result<(), Box<dyn std::error::Error>> {
    let did = ensure_rw()?;
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, path)?;
    if did {
        let _ = Command::new("sync").output();
        finish_write(true);
    }
    Ok(())
}

// ── Make readonly / writable (config flag only — fstab is set by postinst) ─

pub fn make_readonly() -> Result<(), String> {
    let _ = Command::new("sync").output();
    Ok(())
}

pub fn make_writable() -> Result<(), String> {
    let _ = Command::new("sync").output();
    Ok(())
}
