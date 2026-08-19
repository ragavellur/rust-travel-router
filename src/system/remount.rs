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

// ── fstab manipulation ─────────────────────────────────────────────────

const NM_TMPFS_ENTRIES: &[&str] = &[
    "tmpfs /tmp tmpfs defaults,nosuid,nodev,size=100M 0 0",
    "tmpfs /var/tmp tmpfs defaults,nosuid,nodev,size=50M 0 0",
    "tmpfs /var/log tmpfs defaults,nosuid,nodev,size=50M 0 0",
    "tmpfs /var/run tmpfs defaults,nosuid,nodev,size=10M 0 0",
    "tmpfs /var/lock tmpfs defaults,nosuid,nodev,size=5M 0 0",
    "tmpfs /var/lib/NetworkManager tmpfs defaults,nosuid,nodev,size=20M,mode=755 0 0",
    "tmpfs /etc/NetworkManager/system-connections tmpfs defaults,nosuid,nodev,size=5M,mode=755 0 0",
    "tmpfs /etc/NetworkManager/dnsmasq-shared.d tmpfs defaults,nosuid,nodev,size=1M,mode=755 0 0",
    "tmpfs /etc/NetworkManager/conf.d tmpfs defaults,nosuid,nodev,size=1M,mode=755 0 0",
];

const HOSTAPD_TMPFS_ENTRIES: &[&str] = &[
    "tmpfs /tmp tmpfs defaults,nosuid,nodev,size=100M 0 0",
    "tmpfs /var/tmp tmpfs defaults,nosuid,nodev,size=50M 0 0",
    "tmpfs /var/log tmpfs defaults,nosuid,nodev,size=50M 0 0",
    "tmpfs /var/run tmpfs defaults,nosuid,nodev,size=10M 0 0",
    "tmpfs /var/lock tmpfs defaults,nosuid,nodev,size=5M 0 0",
    "tmpfs /etc/hostapd tmpfs defaults,nosuid,nodev,size=5M,mode=755 0 0",
    "tmpfs /etc/wpa_supplicant tmpfs defaults,nosuid,nodev,size=5M,mode=755 0 0",
    "tmpfs /etc/dnsmasq.d tmpfs defaults,nosuid,nodev,size=5M,mode=755 0 0",
    "tmpfs /var/lib/dnsmasq tmpfs defaults,nosuid,nodev,size=5M,mode=755 0 0",
];

fn detect_backend() -> String {
    if Command::new("nmcli").output().is_ok()
        && Command::new("NetworkManager").output().is_ok()
    {
        "networkmanager".to_string()
    } else {
        "hostapd".to_string()
    }
}

fn ensure_dir(path: &str) {
    let _ = std::fs::create_dir_all(path);
}

fn ensure_mount_dirs(backend: &str) {
    // Common dirs
    ensure_dir("/tmp");
    ensure_dir("/var/tmp");
    ensure_dir("/var/log");
    ensure_dir("/var/run");
    ensure_dir("/var/lock");

    if backend == "networkmanager" {
        ensure_dir("/var/lib/NetworkManager");
        ensure_dir("/etc/NetworkManager/system-connections");
        ensure_dir("/etc/NetworkManager/dnsmasq-shared.d");
        ensure_dir("/etc/NetworkManager/conf.d");
    } else {
        ensure_dir("/etc/hostapd");
        ensure_dir("/etc/wpa_supplicant");
        ensure_dir("/etc/dnsmasq.d");
        ensure_dir("/var/lib/dnsmasq");
    }
}

fn add_fstab_entries(backend: &str) -> Result<(), String> {
    let fstab = std::fs::read_to_string("/etc/fstab").map_err(|e| format!("Read fstab: {e}"))?;
    let entries = if backend == "networkmanager" {
        NM_TMPFS_ENTRIES
    } else {
        HOSTAPD_TMPFS_ENTRIES
    };

    let mut new_lines: Vec<String> = fstab.lines().map(|l| l.to_string()).collect();

    for entry in entries {
        let mount_point = entry.split_whitespace().nth(1).unwrap_or("");
        // Skip if already present
        if new_lines.iter().any(|l| l.starts_with(&format!("tmpfs {mount_point} "))) {
            continue;
        }
        new_lines.push(entry.to_string());
    }

    let new_fstab = new_lines.join("\n") + "\n";
    std::fs::write("/etc/fstab", &new_fstab).map_err(|e| format!("Write fstab: {e}"))?;
    Ok(())
}

fn remove_fstab_entries() -> Result<(), String> {
    let fstab = std::fs::read_to_string("/etc/fstab").map_err(|e| format!("Read fstab: {e}"))?;
    let new_fstab: String = fstab
        .lines()
        .filter(|line| !line.starts_with("tmpfs /tmp ") && !line.starts_with("tmpfs /var/tmp ")
            && !line.starts_with("tmpfs /var/log ") && !line.starts_with("tmpfs /var/run ")
            && !line.starts_with("tmpfs /var/lock ") && !line.starts_with("tmpfs /var/lib/NetworkManager ")
            && !line.starts_with("tmpfs /etc/NetworkManager/system-connections ")
            && !line.starts_with("tmpfs /etc/NetworkManager/dnsmasq-shared.d ")
            && !line.starts_with("tmpfs /etc/NetworkManager/conf.d ")
            && !line.starts_with("tmpfs /etc/hostapd ") && !line.starts_with("tmpfs /etc/wpa_supplicant ")
            && !line.starts_with("tmpfs /etc/dnsmasq.d ") && !line.starts_with("tmpfs /var/lib/dnsmasq "))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write("/etc/fstab", &(new_fstab + "\n")).map_err(|e| format!("Write fstab: {e}"))?;
    Ok(())
}

fn set_fstab_ro() -> Result<(), String> {
    let fstab = std::fs::read_to_string("/etc/fstab").map_err(|e| format!("Read fstab: {e}"))?;
    let new_fstab: String = fstab
        .lines()
        .map(|line| {
            if (line.starts_with("UUID=") || line.starts_with("/dev/"))
                && line.contains(" / ext4 ")
                && !line.contains(",ro")
                && !line.starts_with("ro,")
            {
                line.replacen("defaults", "ro,defaults", 1)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write("/etc/fstab", &new_fstab).map_err(|e| format!("Write fstab: {e}"))?;
    Ok(())
}

fn remove_fstab_ro() -> Result<(), String> {
    let fstab = std::fs::read_to_string("/etc/fstab").map_err(|e| format!("Read fstab: {e}"))?;
    let new_fstab: String = fstab
        .lines()
        .map(|line| {
            if (line.starts_with("UUID=") || line.starts_with("/dev/"))
                && line.contains(" / ext4 ")
            {
                line.replace("ro,defaults", "defaults")
                    .replace(",ro,", ",")
                    .replace(",ro,", ",")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write("/etc/fstab", &new_fstab).map_err(|e| format!("Write fstab: {e}"))?;
    Ok(())
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

/// Enable RO protection: add tmpfs fstab entries, set rootfs ro, reboot.
pub fn make_readonly() -> Result<(), String> {
    let backend = detect_backend();
    ensure_mount_dirs(&backend);
    add_fstab_entries(&backend)?;
    set_fstab_ro()?;
    let _ = Command::new("sync").output();
    Ok(())
}

/// Disable RO protection: remove tmpfs fstab entries, remove ro, reboot.
pub fn make_writable() -> Result<(), String> {
    remove_fstab_entries()?;
    remove_fstab_ro()?;
    let _ = Command::new("sync").output();
    Ok(())
}
