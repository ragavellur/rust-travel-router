use std::process::Command;

#[allow(dead_code)]
const OVERLAY_DIRS: &[&str] = &[
    "/etc/travel-net",
    "/etc/NetworkManager/system-connections",
    "/etc/NetworkManager/dnsmasq-shared.d",
    "/etc/hostapd",
    "/etc/wpa_supplicant",
    "/etc/dnsmasq.d",
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

pub fn is_rootfs_readonly() -> bool {
    Command::new("mountpoint")
        .args(["-q", "/"])
        .output()
        .map(|o| !o.status.success())
        .unwrap_or_else(|_| {
            Command::new("awk")
                .args(["$", "/ \\//{print $6; exit}", "/etc/fstab"])
                .output()
                .map(|o| {
                    let val = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    val.contains("ro") && !val.contains("rw")
                })
                .unwrap_or(false)
        })
}

pub fn make_writable() -> Result<(), String> {
    // Update fstab: replace ro with rw for rootfs
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
    std::fs::write("/etc/fstab", &new_fstab).map_err(|e| format!("Write fstab: {e}"))?;

    // Remount rootfs rw
    let output = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output()
        .map_err(|e| format!("mount remount,rw: {e}"))?;
    if !output.status.success() {
        return Err("Failed to remount rootfs rw".into());
    }

    Ok(())
}

pub fn make_readonly() -> Result<(), String> {
    // Sync before going ro
    let _ = Command::new("sync").output();

    // Update fstab: add ro for rootfs
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
    std::fs::write("/etc/fstab", &new_fstab).map_err(|e| format!("Write fstab: {e}"))?;

    // Remount rootfs ro
    let output = Command::new("mount")
        .args(["-o", "remount,ro", "/"])
        .output()
        .map_err(|e| format!("mount remount,ro: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let msg = format!("mount -o remount,ro / failed: stderr={stderr} stdout={stdout}");
        tracing::error!("{msg}");
        eprintln!("{msg}");
        // Try to find what's holding files open
        let fuser = Command::new("fuser").args(["-vm", "/"]).output();
        if let Ok(out) = fuser {
            let fuser_out = String::from_utf8_lossy(&out.stdout).trim().to_string();
            tracing::error!("fuser / : {fuser_out}");
        }
        return Err(msg);
    }

    // Enable overlay service for next boot
    let _ = Command::new("systemctl")
        .args(["enable", "travel-net-overlay.service"])
        .output();

    // Persist settings to disk while still rw (before remount)
    persist_config();
    persist_nm_connections();

    Ok(())
}

fn persist_dir(src: &str) -> Result<(), String> {
    let tmpdir = tmpdir_for(src);

    let is_mounted = Command::new("mountpoint")
        .args(["-q", src])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_mounted {
        return Ok(());
    }

    let unbind = Command::new("umount").arg(src).output();
    if !unbind.map(|o| o.status.success()).unwrap_or(false) {
        return Err(format!("Failed to unbind {src}"));
    }

    let rw = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output()
        .map_err(|e| format!("remount rw: {e}"))?;
    if !rw.status.success() {
        let _ = Command::new("mount").args(["--bind", &tmpdir, src]).output();
        return Err("Failed to remount rootfs rw".into());
    }

    let cp = Command::new("cp")
        .args(["-a", &format!("{tmpdir}/."), src])
        .output()
        .map_err(|e| format!("cp: {e}"))?;
    if !cp.status.success() {
        let err = String::from_utf8_lossy(&cp.stderr);
        tracing::warn!("persist cp error for {src}: {err}");
    }

    let _ = Command::new("sync").output();

    let ro = Command::new("mount")
        .args(["-o", "remount,ro", "/"])
        .output()
        .map_err(|e| format!("remount ro: {e}"))?;
    if !ro.status.success() {
        tracing::warn!("Failed to remount rootfs ro for {src}");
    }

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

#[allow(dead_code)]
pub fn persist_all() {
    for dir in OVERLAY_DIRS {
        if let Err(e) = persist_dir(dir) {
            tracing::warn!("Failed to persist {dir}: {e}");
        }
    }
}
