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
        PipeRequest::GetCapabilities => match read_snapshot(paths.worker_snapshot("capabilities")) {
            Ok(payload) => PipeResponse::Ok { payload },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
        },
        PipeRequest::GetControlSnapshot => match read_snapshot(paths.worker_snapshot("control")) {
            Ok(payload) => PipeResponse::Ok { payload },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
        },
        PipeRequest::GetTelemetrySnapshot => match read_snapshot(paths.worker_snapshot("telemetry")) {
            Ok(payload) => PipeResponse::Ok { payload },
            Err(error) => PipeResponse::Error {
                message: error.to_string(),
            },
        },
        PipeRequest::ApplyPowerProfile { payload } => {
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
        PipeRequest::ApplyGpuTuning { payload } => match control::apply_gpu_tuning(paths, payload) {
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
        },
        PipeRequest::ApplyFanProfile { payload } => match control::apply_fan_profile(paths, payload) {
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
        },
        PipeRequest::ApplyCustomFanCurves { payload } => {
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
        PipeRequest::ApplyBootLogo { payload } => match control::apply_boot_logo(paths, payload) {
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
        },
        PipeRequest::ApplySmartCharging { payload } => {
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
        PipeRequest::ApplyTelemetrySettings { payload } => {
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
