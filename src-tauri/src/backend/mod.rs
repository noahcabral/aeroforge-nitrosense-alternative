pub mod app_rules;
pub mod blue_light;
pub mod boot_logo;
pub mod commands;
pub mod cpu_clock;
pub mod display_refresh;
pub mod hardware_readiness;
pub mod log_file;
pub mod models;
pub mod nitro_guard;
pub mod nitro_key;
pub mod process;
pub mod service_pipe;
pub mod single_instance;
pub mod smart_charge;
pub mod state;
pub mod tray;
pub mod updater;

use std::time::{SystemTime, UNIX_EPOCH};

/// The one error type every fallible path in this crate boxes into.
pub type DynError = Box<dyn std::error::Error + Send + Sync>;

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
