use std::fs;
use std::time::Duration;

pub fn get_uptime() -> Duration {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(Duration::from_secs_f64)
        .unwrap_or(Duration::ZERO)
}
