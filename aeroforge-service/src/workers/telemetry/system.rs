use std::sync::atomic::{AtomicU64, Ordering};

static HEARTBEAT: AtomicU64 = AtomicU64::new(0);

pub fn next_heartbeat() -> u64 {
    HEARTBEAT.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn select_system_temp_c(thermal_zone_temp_c: Option<u8>) -> Option<u8> {
    let zone = thermal_zone_temp_c?;

    if !(20..=95).contains(&zone) {
        return None;
    }

    Some(zone.saturating_sub(2))
}

pub fn sanitize_log_message(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}
