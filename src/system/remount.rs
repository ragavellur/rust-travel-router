use std::process::Command;

// ── Rootfs state detection ──────────────────────────────────────────────

pub fn is_rootfs_readonly() -> bool {
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[1] == "/" {
                let opts = &parts[3];
                return opts.contains(",ro") || opts.starts_with("ro,");
            }
        }
    }
    false
}

// ── Safe filesystem helpers ─────────────────────────────────────────────

pub fn safe_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content)
        .map_err(|e| format!("Write {}: {e}", path.display()))
}

pub fn safe_create_dir_all(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(path)
        .map_err(|e| format!("Mkdir {}: {e}", path.display()))
}

pub fn safe_set_permissions(path: &std::path::Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("Chmod {}: {e}", path.display()))
}

pub fn save_config(
    path: &std::path::Path,
    cfg: &crate::config::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, path)?;
    let _ = Command::new("sync").output();
    Ok(())
}

// ── Placeholder for future RO rootfs support ────────────────────────────

pub fn make_readonly() -> Result<(), String> {
    Err("Read-only rootfs is not yet supported in this version".into())
}

#[allow(dead_code)]
pub fn make_writable() -> Result<(), String> {
    Ok(())
}
