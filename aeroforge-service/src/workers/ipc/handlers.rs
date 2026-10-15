use std::{fs, path::Path, sync::Mutex};

use serde_json::{json, Value};

use super::models::{PipeRequest, PipeResponse, SupervisorSnapshot};
use crate::DynError;
use crate::{
    paths::ServicePaths,
    workers::{control, unix_timestamp},
};

const FRESH_PERIODIC_WORKER_MAX_AGE_SECONDS: u64 = 15;

pub fn process_request(
    request: PipeRequest,
    paths: &ServicePaths,
    pipe_path: &str,
) -> PipeResponse {
    match request {
        PipeRequest::GetServiceStatus => respond(build_service_status(paths, pipe_path)),
        PipeRequest::GetCapabilities => {
            respond(read_snapshot(paths.worker_snapshot("capabilities")))
        }
        PipeRequest::GetControlSnapshot => respond(read_snapshot(paths.worker_snapshot("control"))),
        PipeRequest::GetTelemetrySnapshot => {
            respond(read_snapshot(paths.worker_snapshot("telemetry")))
        }
        PipeRequest::RestoreStartupState { write_token } => {
            authorized_write(paths, write_token, || {
                control::restore_startup_state(paths).map(|()| {
                    json!({
                        "detail": "Restored saved AeroForge power and fan state on manual app launch."
                    })
                })
            })
        }
        PipeRequest::ApplyPowerProfile {
            payload,
            write_token,
        } => authorized_write(paths, write_token, || {
            control::apply_power_profile(paths, payload)
                .map(|applied| serialize_applied("power profile", applied))
        }),
        PipeRequest::ApplyGpuTuning {
            payload,
            write_token,
        } => authorized_write(paths, write_token, || {
            control::apply_gpu_tuning(paths, payload)
                .map(|applied| serialize_applied("GPU tuning", applied))
        }),
        PipeRequest::ApplyFanProfile {
            payload,
            write_token,
        } => authorized_write(paths, write_token, || {
            control::apply_fan_profile(paths, payload)
                .map(|applied| serialize_applied("fan profile", applied))
        }),
        PipeRequest::ApplyCustomFanCurves {
            payload,
            write_token,
        } => authorized_write(paths, write_token, || {
            control::apply_custom_fan_curves(paths, payload)
                .map(|applied| serialize_applied("custom fan curves", applied))
        }),
        PipeRequest::CancelFanSpeedCalibration { write_token } => {
            authorized_write(paths, write_token, || {
                control::cancel_fan_speed_calibration(paths)
                    .map(|calibration| serialize_applied("fan speed calibration", calibration))
            })
        }
        PipeRequest::StartFanSpeedCalibration { write_token } => {
            authorized_write(paths, write_token, || {
                control::start_fan_speed_calibration(paths)
                    .map(|calibration| serialize_applied("fan speed calibration", calibration))
            })
        }
        PipeRequest::ApplyBootLogo {
            payload,
            write_token,
        } => authorized_write(paths, write_token, || {
            control::apply_boot_logo(paths, payload)
                .map(|applied| serialize_applied("boot logo", applied))
        }),
        PipeRequest::ApplySmartCharging {
            payload,
            write_token,
        } => authorized_write(paths, write_token, || {
            control::apply_smart_charging(paths, payload)
                .map(|applied| serialize_applied("smart charging", applied))
        }),
        PipeRequest::ApplyTelemetrySettings {
            payload,
            write_token,
        } => authorized_write(paths, write_token, || {
            control::apply_telemetry_settings(paths, payload)
                .map(|applied| serialize_applied("telemetry settings", applied))
        }),
    }
}

fn respond(result: Result<Value, DynError>) -> PipeResponse {
    match result {
        Ok(payload) => PipeResponse::Ok { payload },
        Err(error) => PipeResponse::Error {
            message: error.to_string(),
        },
    }
}

/// Every hardware write is gated on the TOFU token before the handler runs.
fn authorized_write(
    paths: &ServicePaths,
    write_token: Option<String>,
    apply: impl FnOnce() -> Result<Value, DynError>,
) -> PipeResponse {
    if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
        return PipeResponse::Error { message };
    }
    respond(apply())
}

/// The hardware change already happened by the time we serialize, so a serde
/// failure is reported as a detail rather than turned into an error response.
fn serialize_applied(what: &str, applied: impl serde::Serialize) -> Value {
    serde_json::to_value(applied).unwrap_or_else(|error| {
        json!({
            "detail": format!("Applied {what} but failed to serialize response: {error}")
        })
    })
}

// Clients are handled concurrently, so the first-use check and the token write
// happen under one lock: two first clients can never both see "no token yet".
static TOKEN_LOCK: Mutex<()> = Mutex::new(());

fn require_write_authorization(paths: &ServicePaths, token: Option<&str>) -> Result<(), String> {
    let Some(token) = token.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(
            "Hardware write denied: AeroForge desktop did not provide an IPC write token.".into(),
        );
    };
    if !is_valid_write_token(token) {
        return Err("Hardware write denied: IPC write token shape was invalid.".into());
    }

    let _guard = TOKEN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let token_path = paths.ipc_write_token();
    if let Ok(existing) = fs::read_to_string(&token_path) {
        let existing = existing.trim();
        // Reclaim when the on-disk token is missing shape (corrupt install / partial wipe).
        if existing.is_empty() || !is_valid_write_token(existing) {
            write_token_file(paths, token)?;
            return Ok(());
        }
        if constant_time_eq(existing.as_bytes(), token.as_bytes()) {
            return Ok(());
        }
        return Err(
            "Hardware write denied: IPC write token did not match this service installation. Repair or reinstall AeroForge if the desktop and service tokens are out of sync."
                .into(),
        );
    }

    write_token_file(paths, token)?;
    Ok(())
}

fn write_token_file(paths: &ServicePaths, token: &str) -> Result<(), String> {
    let token_path = paths.ipc_write_token();
    crate::paths::write_text_atomically(&token_path, token).map_err(|error| error.to_string())?;
    // Best effort: the service root ACL is re-applied to the pin at every start.
    if let Err(error) = crate::paths::restrict_to_system_and_admins(&token_path) {
        let _ = crate::paths::write_log_line(
            &paths.service_log(),
            "WARN",
            &format!("Could not restrict IPC write token permissions: {error}"),
        );
    }
    Ok(())
}

fn is_valid_write_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Constant-time equality for same-length secrets.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let mut diff = 0u8;
    for index in 0..left.len() {
        diff |= left[index] ^ right[index];
    }
    diff == 0
}

fn build_service_status(paths: &ServicePaths, pipe_path: &str) -> Result<Value, DynError> {
    let supervisor_raw = fs::read_to_string(paths.supervisor_snapshot())?;
    let supervisor = serde_json::from_str::<SupervisorSnapshot>(&supervisor_raw)?;
    let worker_problem = critical_worker_problem(&supervisor.workers);
    let connected = worker_problem.is_none();
    let detail = if let Some(problem) = worker_problem {
        format!("Connected to the AeroForge service named-pipe host, but service controls are degraded: {problem}.")
    } else {
        "Connected to the AeroForge service named-pipe host.".into()
    };

    Ok(json!({
        "connected": connected,
        "pipeName": pipe_path,
        "serviceName": supervisor.service,
        "version": env!("CARGO_PKG_VERSION"),
        "stateDir": paths.state_dir.display().to_string(),
        "supervisorFile": paths.supervisor_snapshot().display().to_string(),
        "workerCount": supervisor.worker_count,
        "updatedAtUnix": supervisor.updated_at_unix,
        "workers": supervisor.workers,
        "detail": detail
    }))
}

fn critical_worker_problem(workers: &[super::models::WorkerStatusSnapshot]) -> Option<String> {
    let now_unix = unix_timestamp();

    for worker_name in ["control-worker", "ipc-worker"] {
        let Some(worker) = workers.iter().find(|worker| worker.name == worker_name) else {
            return Some(format!(
                "{worker_name} is missing from the supervisor snapshot"
            ));
        };
        let state = worker.state.trim().to_ascii_lowercase();
        if !matches!(state.as_str(), "running" | "starting") {
            return Some(format!(
                "{} is {}{}",
                worker.name,
                worker.state,
                worker
                    .last_error
                    .as_deref()
                    .map(|error| format!(" ({error})"))
                    .unwrap_or_default()
            ));
        }

        if worker.interval_seconds > 0 {
            let max_age = FRESH_PERIODIC_WORKER_MAX_AGE_SECONDS
                .max(worker.interval_seconds.saturating_mul(4));
            let age = now_unix.saturating_sub(worker.last_update_unix);
            if age > max_age {
                return Some(format!(
                    "{} heartbeat is stale: last update was {age}s ago, expected within {max_age}s",
                    worker.name
                ));
            }
        }
    }

    None
}

fn read_snapshot(path: impl AsRef<Path>) -> Result<Value, DynError> {
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str::<Value>(
        raw.trim_start_matches('\u{feff}'),
    )?)
}

#[cfg(test)]
mod tests {
    use super::{is_valid_write_token, process_request};
    use crate::{
        paths::ServicePaths,
        workers::ipc::models::{PipeRequest, PipeResponse},
    };

    #[test]
    fn writes_without_a_token_are_denied_before_touching_hardware() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = ServicePaths {
            logs_dir: temp.path().join("logs"),
            state_dir: temp.path().join("state"),
        };
        let response = process_request(
            PipeRequest::RestoreStartupState { write_token: None },
            &paths,
            "pipe",
        );
        match response {
            PipeResponse::Error { message } => {
                assert!(message.contains("did not provide"), "{message}")
            }
            PipeResponse::Ok { .. } => panic!("write must be denied without a token"),
        }
        assert!(!paths.state_dir.join("ipc-write-token.txt").exists());
    }

    #[test]
    fn write_authorization_accepts_only_hex_tokens() {
        assert!(is_valid_write_token(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_valid_write_token("short"));
        assert!(!is_valid_write_token(
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        ));
    }
}
