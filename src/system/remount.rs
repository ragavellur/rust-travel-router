use std::process::Command;

pub fn remount_rw() {
    let _ = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output();
}

pub fn with_writable_rootfs<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    remount_rw();
    f()
}
