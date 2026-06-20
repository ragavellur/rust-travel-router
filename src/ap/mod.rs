pub mod hostapd;
pub mod interface;
pub mod apply;

use crate::config::Config;

pub async fn start_ap(cfg: &Config) -> Result<(), String> {
    interface::create_ap_interface(cfg).await?;
    hostapd::start_hostapd(cfg).await?;
    Ok(())
}
