use std::process::Command;

const NM_CONNECTIONS: &str = "/etc/NetworkManager/system-connections";
const TC_CONFIG: &str = "/etc/travel-net";

fn tmpdir_for(path: &str) -> String {
    format!("/tmp/overlay-{}", path.replace('/', "_").trim_start_matches('_'))
}

fn persist_dir(src: &str) -> Result<(), String> {
    let tmpdir = tmpdir_for(src);

    // Unbind tmpfs (expose real ext4 directory)
    let unbind = Command::new("umount").arg(src).output();
    if !unbind.map(|o| o.status.success()).unwrap_or(false) {
        // Not a bind mount — nothing to persist
        return Ok(());
    }

    // Remount rootfs rw
    Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output()
        .map_err(|e| format!("remount rw: {e}"))?;

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
    Command::new("mount")
        .args(["-o", "remount,ro", "/"])
        .output()
        .map_err(|e| format!("remount ro: {e}"))?;

    // Re-bind tmpfs
    Command::new("mount")
        .args(["--bind", &tmpdir, src])
        .output()
        .map_err(|e| format!("rebind: {e}"))?;

    tracing::info!("Persisted {src} to disk");
    Ok(())
}

pub fn persist_config() {
    if let Err(e) = persist_dir(TC_CONFIG) {
        tracing::warn!("Failed to persist config: {e}");
    }
}

pub fn persist_nm_connections() {
    if let Err(e) = persist_dir(NM_CONNECTIONS) {
        tracing::warn!("Failed to persist NM connections: {e}");
    }
}

pub fn persist_all() {
    persist_config();
    persist_nm_connections();
}
