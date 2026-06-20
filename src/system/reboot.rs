use std::process::Command;

pub fn reboot() -> Result<(), String> {
    Command::new("reboot")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Reboot failed: {e}"))
}
