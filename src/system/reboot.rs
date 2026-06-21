use std::process::Command;

pub fn reboot() -> Result<(), String> {
    Command::new("reboot")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Reboot failed: {e}"))
}

pub fn shutdown() -> Result<(), String> {
    Command::new("poweroff")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Shutdown failed: {e}"))
}
