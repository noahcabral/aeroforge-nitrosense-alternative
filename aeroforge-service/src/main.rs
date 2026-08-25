mod host;
mod paths;
mod workers;

use std::ffi::OsString;

use host::{run_console_host, run_windows_service_host};

const SERVICE_NAME: &str = "AeroForgeService";

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let console_mode = args.iter().any(|arg| arg == "--console");

    if console_mode {
        return run_console_host();
    }

    run_windows_service_host(SERVICE_NAME)
}
