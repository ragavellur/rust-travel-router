use std::process::Command;

pub fn remount_rw() {
    let _ = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .output();
}

pub fn remount_ro() {
    let _ = Command::new("mount")
        .args(["-o", "remount,ro", "/"])
        .output();
}

pub fn with_writable_rootfs<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    remount_rw();
    let result = f();
    remount_ro();
    result
}
