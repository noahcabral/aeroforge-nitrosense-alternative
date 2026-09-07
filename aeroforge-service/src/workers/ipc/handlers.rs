use std::{fs, path::Path};

use serde_json::{json, Value};

use super::models::{PipeRequest, PipeResponse, SupervisorSnapshot};
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
        PipeRequest::GetServiceStatus => match build_service_status(paths, pipe_path) {
            Ok(payload) => PipeResponse::Ok { payload },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
        },
        PipeRequest::GetCapabilities => {
            match read_snapshot(paths.worker_snapshot("capabilities")) {
                Ok(payload) => PipeResponse::Ok { payload },
                Err(error) => PipeResponse::Error {
                    message: error.to_string(),
                },
            }
        }
        PipeRequest::GetControlSnapshot => match read_snapshot(paths.worker_snapshot("control")) {
            Ok(payload) => PipeResponse::Ok { payload },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
        },
        PipeRequest::GetTelemetrySnapshot => {
            match read_snapshot(paths.worker_snapshot("telemetry")) {
                Ok(payload) => PipeResponse::Ok { payload },
                Err(error) => PipeResponse::Error {
                    message: error.to_string(),
                },
            }
        }
        PipeRequest::RestoreStartupState { write_token } => {
            match require_write_authorization(paths, write_token.as_deref()).and_then(|_| {
                control::restore_startup_state(paths).map_err(|error| error.to_string())
            }) {
                Ok(()) => PipeResponse::Ok {
                    payload: json!({
                        "detail": "Restored saved AeroForge power and fan state on manual app launch."
                    }),
                },
                Err(error) => PipeResponse::Error { message: error },
            }
        }
        PipeRequest::ApplyPowerProfile {
            payload,
            write_token,
        } => {
            if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
                return PipeResponse::Error { message };
            }
            match control::apply_power_profile(paths, payload) {
                Ok(applied) => PipeResponse::Ok {
                    payload: serde_json::to_value(applied).unwrap_or_else(|error| {
                        json!({
                            "detail": format!("Applied power profile but failed to serialize response: {error}")
                        })
                    }),
                },
                Err(error) => PipeResponse::Error {
                    message: error.to_string(),
                },
            }
        }
        PipeRequest::ApplyGpuTuning {
            payload,
            write_token,
        } => {
            if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
                return PipeResponse::Error { message };
            }
            match control::apply_gpu_tuning(paths, payload) {
            Ok(applied) => PipeResponse::Ok {
                payload: serde_json::to_value(applied).unwrap_or_else(|error| {
                    json!({
                        "detail": format!("Applied GPU tuning but failed to serialize response: {error}")
                    })
                }),
            },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
            }
        }
        PipeRequest::ApplyFanProfile {
            payload,
            write_token,
        } => {
            if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
                return PipeResponse::Error { message };
            }
            match control::apply_fan_profile(paths, payload) {
            Ok(applied) => PipeResponse::Ok {
                payload: serde_json::to_value(applied).unwrap_or_else(|error| {
                    json!({
                        "detail": format!("Applied fan profile but failed to serialize response: {error}")
                    })
                }),
            },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
            }
        }
        PipeRequest::ApplyCustomFanCurves {
            payload,
            write_token,
        } => {
            if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
                return PipeResponse::Error { message };
            }
            match control::apply_custom_fan_curves(paths, payload) {
                Ok(applied) => PipeResponse::Ok {
                    payload: serde_json::to_value(applied).unwrap_or_else(|error| {
                        json!({
                            "detail": format!("Applied custom fan curves but failed to serialize response: {error}")
                        })
                    }),
                },
                Err(error) => PipeResponse::Error {
                    message: error.to_string(),
                },
            }
        }
        PipeRequest::ApplyBootLogo {
            payload,
            write_token,
        } => {
            if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
                return PipeResponse::Error { message };
            }
            match control::apply_boot_logo(paths, payload) {
            Ok(applied) => PipeResponse::Ok {
                payload: serde_json::to_value(applied).unwrap_or_else(|error| {
                    json!({
                        "detail": format!("Applied boot logo but failed to serialize response: {error}")
                    })
                }),
            },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
            }
        }
        PipeRequest::ApplySmartCharging {
            payload,
            write_token,
        } => {
            if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
                return PipeResponse::Error { message };
            }
            match control::apply_smart_charging(paths, payload) {
                Ok(applied) => PipeResponse::Ok {
                    payload: serde_json::to_value(applied).unwrap_or_else(|error| {
                        json!({
                            "detail": format!("Applied smart charging but failed to serialize response: {error}")
                        })
                    }),
                },
                Err(error) => PipeResponse::Error {
                    message: error.to_string(),
                },
            }
        }
        PipeRequest::ApplyTelemetrySettings {
            payload,
            write_token,
        } => {
            if let Err(message) = require_write_authorization(paths, write_token.as_deref()) {
                return PipeResponse::Error { message };
            }
            match control::apply_telemetry_settings(paths, payload) {
                Ok(applied) => PipeResponse::Ok {
                    payload: serde_json::to_value(applied).unwrap_or_else(|error| {
                        json!({
                            "detail": format!("Applied telemetry settings but failed to serialize response: {error}")
                        })
                    }),
                },
                Err(error) => PipeResponse::Error {
                    message: error.to_string(),
                },
            }
        }
    }
}

fn require_write_authorization(paths: &ServicePaths, token: Option<&str>) -> Result<(), String> {
    let Some(token) = token.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(
            "Hardware write denied: AeroForge desktop did not provide an IPC write token.".into(),
        );
    };
    if !is_valid_write_token(token) {
        return Err("Hardware write denied: IPC write token shape was invalid.".into());
    }

    let token_path = paths.state_dir.join("ipc-write-token.txt");
    if let Ok(existing) = fs::read_to_string(&token_path) {
        let existing = existing.trim();
        // Reclaim when the on-disk token is missing shape (corrupt install / partial wipe).
        if existing.is_empty() || !is_valid_write_token(existing) {
            write_token_file(paths, &token_path, token)?;
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

    write_token_file(paths, &token_path, token)?;
    Ok(())
}

fn write_token_file(
    paths: &ServicePaths,
    token_path: &std::path::Path,
    token: &str,
) -> Result<(), String> {
    fs::create_dir_all(&paths.state_dir).map_err(|error| error.to_string())?;
    let temp_path = paths.state_dir.join(format!("ipc-write-token.{}.tmp", std::process::id()));
    fs::write(&temp_path, token).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&temp_path, token_path) {
        let _ = fs::remove_file(&temp_path);
        // Another client may have created the token between our read and write.
        if let Ok(existing) = fs::read_to_string(token_path) {
            if constant_time_eq(existing.trim().as_bytes(), token.as_bytes()) {
                return Ok(());
            }
            return Err(
                "Hardware write denied: IPC write token did not match this service installation. Repair or reinstall AeroForge if the desktop and service tokens are out of sync."
                    .into(),
            );
        }
        return Err(error.to_string());
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

fn build_service_status(
    paths: &ServicePaths,
    pipe_path: &str,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
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

fn read_snapshot(
    path: impl AsRef<Path>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str::<Value>(
        raw.trim_start_matches('\u{feff}'),
    )?)
}

#[cfg(test)]
mod tests {
    use super::is_valid_write_token;

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
