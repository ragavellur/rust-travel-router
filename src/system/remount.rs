use std::process::Command;

const OVERLAY_DIRS: &[&str] = &[
    "/etc/travel-net",
    "/etc/NetworkManager/system-connections",
    "/etc/hostapd",
    "/etc/wpa_supplicant",
];

fn tmpdir_for(path: &str) -> String {
    format!(
        "/tmp/overlay-{}",
        path.replace('/', "_").trim_start_matches('_')
    )
}

fn persist_dir(src: &str) -> Result<(), String> {
    let tmpdir = tmpdir_for(src);

    // Check if this directory is bind-mounted (overlay active)
    let is_mounted = Command::new("mountpoint")
        .args(["-q", src])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_mounted {
        // No overlay — write directly to real fs
        return Ok(());
    }

    // Unbind tmpfs (expose real ext4 directory)
    let unbind = Command::new("umount").arg(src).output();
    if !unbind.map(|o| o.status.success()).unwrap_or(false) {
        return Err(format!("Failed to unbind {src}"));
    }

    // Remount rootfs rw
    let rw = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output()
        .map_err(|e| format!("remount rw: {e}"))?;
    if !rw.status.success() {
        // Put the bind back before returning
        let _ = Command::new("mount").args(["--bind", &tmpdir, src]).output();
        return Err("Failed to remount rootfs rw".into());
    }

    // Copy from tmpfs to real ext4
    let cp = Command::new("cp")
        .args(["-a", &format!("{tmpdir}/."), src])
        .output()
        .map_err(|e| format!("cp: {e}"))?;
    if !cp.status.success() {
        let err = String::from_utf8_lossy(&cp.stderr);
        tracing::warn!("persist cp error for {src}: {err}");
    }

    // Sync to disk
    let _ = Command::new("sync").output();

    // Remount rootfs ro
    let ro = Command::new("mount")
        .args(["-o", "remount,ro", "/"])
        .output()
        .map_err(|e| format!("remount ro: {e}"))?;
    if !ro.status.success() {
        tracing::warn!("Failed to remount rootfs ro for {src}");
    }

    // Re-bind tmpfs
    Command::new("mount")
        .args(["--bind", &tmpdir, src])
        .output()
        .map_err(|e| format!("rebind: {e}"))?;

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

pub fn persist_all() {
    for dir in OVERLAY_DIRS {
        if let Err(e) = persist_dir(dir) {
            tracing::warn!("Failed to persist {dir}: {e}");
        }
    }
}
