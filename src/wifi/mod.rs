pub mod scan;
pub mod status;
pub mod connect;

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Backend {
    NetworkManager,
    WpaSupplicant,
}

pub fn detect_backend(override_backend: &str) -> Backend {
    match override_backend {
        "wpa_supplicant" => return Backend::WpaSupplicant,
        "networkmanager" => return Backend::NetworkManager,
        _ => {}
    }
    if Path::new("/usr/bin/nmcli").exists()
        && Path::new("/usr/sbin/NetworkManager").exists()
    {
        Backend::NetworkManager
    } else {
        Backend::WpaSupplicant
    }
}
