use std::{fs, path::Path};

use serde_json::{json, Value};

use super::models::{PipeRequest, PipeResponse, SupervisorSnapshot};
use crate::{paths::ServicePaths, workers::control};

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
    }
}

fn build_service_status(
    paths: &ServicePaths,
    pipe_path: &str,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let supervisor_raw = fs::read_to_string(paths.supervisor_snapshot())?;
    let supervisor = serde_json::from_str::<SupervisorSnapshot>(&supervisor_raw)?;

    Ok(json!({
        "connected": true,
        "pipeName": pipe_path,
        "serviceName": supervisor.service,
        "version": env!("CARGO_PKG_VERSION"),
        "stateDir": paths.state_dir.display().to_string(),
        "supervisorFile": paths.supervisor_snapshot().display().to_string(),
        "workerCount": supervisor.worker_count,
        "updatedAtUnix": supervisor.updated_at_unix,
        "workers": supervisor.workers,
        "detail": "Connected to the AeroForge service named-pipe host."
    }))
}

fn read_snapshot(
    path: impl AsRef<Path>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str::<Value>(&raw)?)
}
