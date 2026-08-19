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

fn ensure_rw_for_write(path: &std::path::Path) -> Result<bool, String> {
    if !is_rootfs_readonly() {
        return Ok(false);
    }
    let dir = path.parent().unwrap_or(std::path::Path::new("/"));
    if is_overlay_active(dir.to_str().unwrap_or("")) {
        return Ok(false);
    }
    let output = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output()
        .map_err(|e| format!("mount remount,rw: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("Failed to remount rootfs rw for save: {stderr}"));
    }
    Ok(true)
}

fn restore_ro(did_remount: bool) {
    if !did_remount {
        return;
    }
    let _ = Command::new("sync").output();
    let output = Command::new("mount")
        .args(["-o", "remount,ro", "/"])
        .output();
    if let Ok(out) = output {
        if !out.status.success() {
            tracing::warn!("Failed to restore rootfs to ro after save");
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

pub fn safe_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    if is_file_on_overlay(path) {
        std::fs::write(path, content).map_err(|e| format!("Write {}: {e}", path.display()))?;
        return Ok(());
    }
    let did_rw = ensure_rw_for_write(path)?;
    let result = std::fs::write(path, content)
        .map_err(|e| format!("Write {}: {e}", path.display()));
    restore_ro(did_rw);
    result
}

pub fn safe_write_bytes(path: &std::path::Path, content: &[u8]) -> Result<(), String> {
    if is_file_on_overlay(path) {
        std::fs::write(path, content).map_err(|e| format!("Write {}: {e}", path.display()))?;
        return Ok(());
    }
    let did_rw = ensure_rw_for_write(path)?;
    let result = std::fs::write(path, content)
        .map_err(|e| format!("Write {}: {e}", path.display()));
    restore_ro(did_rw);
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
    let did_rw = ensure_rw_for_write(path)?;
    let result = std::fs::create_dir_all(path)
        .map_err(|e| format!("Mkdir {}: {e}", path.display()));
    restore_ro(did_rw);
    result
}

pub fn safe_set_permissions(path: &std::path::Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if is_file_on_overlay(path) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("Chmod {}: {e}", path.display()))?;
        return Ok(());
    }
    let did_rw = ensure_rw_for_write(path)?;
    let result = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("Chmod {}: {e}", path.display()));
    restore_ro(did_rw);
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

    let did_rw = ensure_rw_for_write(path)?;
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
    restore_ro(did_rw);
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

    let did_rw = ensure_rw_for_write(std::path::Path::new("/etc/fstab"))?;
    let write_result = std::fs::write("/etc/fstab", &new_fstab)
        .map_err(|e| format!("Write fstab: {e}"));

    if write_result.is_err() {
        restore_ro(did_rw);
        return write_result;
    }

    let output = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output()
        .map_err(|e| format!("mount remount,rw: {e}"))?;
    if !output.status.success() {
        restore_ro(did_rw);
        return Err("Failed to remount rootfs rw".into());
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

    let did_rw = ensure_rw_for_write(std::path::Path::new("/etc/fstab"))?;
    let write_result = std::fs::write("/etc/fstab", &new_fstab)
        .map_err(|e| format!("Write fstab: {e}"));

    if write_result.is_err() {
        restore_ro(did_rw);
        return write_result;
    }

    kill_blocking_processes();

    for attempt in 0..3 {
        let output = Command::new("mount")
            .args(["-o", "remount,ro", "/"])
            .output()
            .map_err(|e| format!("mount remount,ro: {e}"))?;
        if output.status.success() {
            break;
        }
        if attempt < 2 {
            tracing::warn!("mount -o remount,ro failed (attempt {}), retrying after kill", attempt + 1);
            kill_blocking_processes();
            std::thread::sleep(std::time::Duration::from_secs(1));
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let fuser_out = Command::new("fuser").args(["-vm", "/"]).output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            tracing::error!("mount -o remount,ro / failed: stderr={stderr} stdout={stdout} fuser={fuser_out}");
            restore_ro(did_rw);
            return Err(format!("mount -o remount,ro / failed: {stderr}"));
        }
    }

    let _ = Command::new("systemctl")
        .args(["enable", "travel-net-overlay.service"])
        .output();

    persist_config();
    persist_nm_connections();

    Ok(())
}

fn kill_blocking_processes() {
    let fuser_out = Command::new("fuser").args(["-vm", "/"]).output();
    if let Ok(out) = fuser_out {
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let pids: Vec<&str> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(pid_str) = parts.last() {
                    let pid = pid_str.trim_end_matches(':');
                    if pid.chars().all(|c| c.is_ascii_digit()) && !pid.is_empty() {
                        return Some(pid);
                    }
                }
                None
            })
            .collect();

        let my_pid = std::process::id().to_string();
        let targets: Vec<&str> = pids.iter().filter(|p| **p != my_pid.as_str()).copied().collect();

        if targets.is_empty() {
            return;
        }

        tracing::info!("Killing {} processes blocking rootfs remount: {}", targets.len(), targets.join(", "));

        let _ = Command::new("kill")
            .args(targets.iter().flat_map(|p| ["-TERM", p]).collect::<Vec<_>>())
            .output();

        std::thread::sleep(std::time::Duration::from_secs(2));

        let still_alive: Vec<String> = targets.iter().filter(|p| {
            std::path::Path::new(&format!("/proc/{}", p)).exists()
        }).map(|p| p.to_string()).collect();

        if !still_alive.is_empty() {
            let _ = Command::new("kill")
                .args(still_alive.iter().flat_map(|p| ["-9", p.as_str()]).collect::<Vec<_>>())
                .output();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
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
