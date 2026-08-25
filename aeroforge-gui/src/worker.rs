use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use aeroforge_common::{ServiceClient, TelemetrySnapshot, ControlSnapshot, ServiceStatus};

const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(1000);
const HIDDEN_POLL_INTERVAL: Duration = Duration::from_millis(5000);
const ERROR_RETRY_INTERVAL: Duration = Duration::from_millis(2000);

pub struct TelemetryWorker {
    telemetry_state: Arc<Mutex<TelemetrySnapshot>>,
    service_state: Arc<Mutex<ServiceStatus>>,
    control_state: Arc<Mutex<ControlSnapshot>>,
    running: Arc<AtomicBool>,
}

impl TelemetryWorker {
    pub fn new(
        telemetry_state: Arc<Mutex<TelemetrySnapshot>>,
        service_state: Arc<Mutex<ServiceStatus>>,
        control_state: Arc<Mutex<ControlSnapshot>>,
        running: Arc<AtomicBool>,
    ) -> Self {
        Self {
            telemetry_state,
            service_state,
            control_state,
            running,
        }
    }

    pub fn run(&self) {
        let mut pipe_error_count = 0;

        while self.running.load(Ordering::Relaxed) {
            let start_time = std::time::Instant::now();
            let mut updated_successfully = false;

            // 1. Try to fetch telemetry
            match ServiceClient::get_telemetry() {
                Ok(telemetry) => {
                    if let Ok(mut state) = self.telemetry_state.lock() {
                        *state = telemetry;
                    }
                    updated_successfully = true;
                    pipe_error_count = 0;

                    // Automatically fetch control state periodically
                    if let Ok(control) = ServiceClient::get_control_snapshot() {
                        if let Ok(mut state) = self.control_state.lock() {
                            *state = control;
                        }
                    }

                    // Fetch service status to keep pipe running smoothly
                    if let Ok(status) = ServiceClient::get_service_status() {
                        if let Ok(mut state) = self.service_state.lock() {
                            *state = status;
                        }
                    }
                }
                Err(e) => {
                    pipe_error_count += 1;

                    // Fall back to cached fast-polling layer if service pipe isn't available
                    // The service keeps C:\ProgramData\AeroForge\Service\state\*.json updated
                    if let Ok(telemetry) = ServiceClient::get_cached_telemetry() {
                        if let Ok(mut state) = self.telemetry_state.lock() {
                            *state = telemetry;
                        }
                    }

                    if let Ok(mut state) = self.service_state.lock() {
                        state.connected = false;
                        state.detail = format!(
                            "AeroForgeService named pipe unavailable: {}. Check if the service is running.",
                            e
                        );
                    }
                }
            }

            // Polling throttle calculation
            // If we are getting pipe errors, slow down polling.
            // In a real app we would also detect window visibility here to use HIDDEN_POLL_INTERVAL.
            let sleep_duration = if updated_successfully {
                ACTIVE_POLL_INTERVAL
            } else {
                ERROR_RETRY_INTERVAL
            };

            let elapsed = start_time.elapsed();
            if elapsed < sleep_duration {
                thread::sleep(sleep_duration - elapsed);
            }
        }
    }
}
