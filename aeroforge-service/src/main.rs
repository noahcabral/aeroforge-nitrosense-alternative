mod host;
mod paths;
mod workers;

/// The one error type every fallible path in this crate boxes into.
pub type DynError = Box<dyn std::error::Error + Send + Sync>;

use std::ffi::OsString;

use host::{run_console_host, run_windows_service_host};

const SERVICE_NAME: &str = "AeroForgeService";

fn main() -> Result<(), DynError> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let console_mode = args.iter().any(|arg| arg == "--console");

    if console_mode {
        return run_console_host();
    }

    run_windows_service_host(SERVICE_NAME)
}
