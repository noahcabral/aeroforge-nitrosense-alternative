use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::models::{
    CapabilitySnapshot, FanCurveSet, FanProfileId, GpuTuningState, LiveControlSnapshot,
    PowerProfileId, ProcessorStateSettings, ServiceStatus, TelemetrySnapshot,
};

const PIPE_PATH: &str = r"\\.\pipe\AeroForgeService";

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PipeRequest {
    GetServiceStatus,
    GetCapabilities,
    GetControlSnapshot,
    GetTelemetrySnapshot,
    ApplyPowerProfile {
        payload: ApplyPowerProfileRequest,
    },
    ApplyGpuTuning {
        payload: ApplyGpuTuningRequest,
    },
    ApplyFanProfile {
        payload: ApplyFanProfileRequest,
    },
    ApplyCustomFanCurves {
        payload: ApplyCustomFanCurvesRequest,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PipeResponse {
    Ok { payload: Value },
    Error { message: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyPowerProfileRequest {
    profile_id: PowerProfileId,
    processor_state: ProcessorStateSettings,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyGpuTuningRequest {
    tuning: GpuTuningState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyFanProfileRequest {
    profile_id: FanProfileId,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyCustomFanCurvesRequest {
    curves: FanCurveSet,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPowerProfilePayload {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedGpuTuningPayload {
    pub tuning: GpuTuningState,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFanControlPayload {
    pub profile_id: FanProfileId,
    pub curves: Option<FanCurveSet>,
    pub applied_at_unix: u64,
    pub detail: String,
}

pub fn pipe_path() -> &'static str {
    PIPE_PATH
}

pub fn fetch_service_status() -> Result<ServiceStatus, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::GetServiceStatus)?;
    Ok(serde_json::from_value::<ServiceStatus>(payload)?)
}

pub fn fetch_capabilities() -> Result<CapabilitySnapshot, Box<dyn std::error::Error + Send + Sync>>
{
    let payload = request(PipeRequest::GetCapabilities)?;
    Ok(serde_json::from_value::<CapabilitySnapshot>(payload)?)
}

pub fn fetch_telemetry() -> Result<TelemetrySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::GetTelemetrySnapshot)?;
    Ok(serde_json::from_value::<TelemetrySnapshot>(payload)?)
}

pub fn fetch_live_controls() -> Result<LiveControlSnapshot, Box<dyn std::error::Error + Send + Sync>>
{
    let payload = request(PipeRequest::GetControlSnapshot)?;
    Ok(serde_json::from_value::<LiveControlSnapshot>(payload)?)
}

pub fn apply_power_profile(
    profile_id: PowerProfileId,
    processor_state: ProcessorStateSettings,
) -> Result<AppliedPowerProfilePayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyPowerProfile {
        payload: ApplyPowerProfileRequest {
            profile_id,
            processor_state,
        },
    })?;
    Ok(serde_json::from_value::<AppliedPowerProfilePayload>(
        payload,
    )?)
}

pub fn apply_gpu_tuning(
    tuning: GpuTuningState,
) -> Result<AppliedGpuTuningPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyGpuTuning {
        payload: ApplyGpuTuningRequest { tuning },
    })?;
    Ok(serde_json::from_value::<AppliedGpuTuningPayload>(payload)?)
}

pub fn apply_fan_profile(
    profile_id: FanProfileId,
) -> Result<AppliedFanControlPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyFanProfile {
        payload: ApplyFanProfileRequest { profile_id },
    })?;
    Ok(serde_json::from_value::<AppliedFanControlPayload>(payload)?)
}

pub fn apply_custom_fan_curves(
    curves: FanCurveSet,
) -> Result<AppliedFanControlPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyCustomFanCurves {
        payload: ApplyCustomFanCurvesRequest { curves },
    })?;
    Ok(serde_json::from_value::<AppliedFanControlPayload>(payload)?)
}

fn request(command: PipeRequest) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut pipe = open_pipe_with_retry()?;
    let serialized = serde_json::to_string(&command)?;
    pipe.write_all(serialized.as_bytes())?;
    pipe.write_all(b"\n")?;
    pipe.flush()?;

    let mut reader = BufReader::new(pipe);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    if line.trim().is_empty() {
        return Err("Named pipe returned an empty response".into());
    }

    match serde_json::from_str::<PipeResponse>(&line)? {
        PipeResponse::Ok { payload } => Ok(payload),
        PipeResponse::Error { message } => Err(message.into()),
    }
}

fn open_pipe_with_retry() -> Result<std::fs::File, Box<dyn std::error::Error + Send + Sync>> {
    let mut last_error: Option<std::io::Error> = None;

    for _ in 0..20 {
        match OpenOptions::new().read(true).write(true).open(PIPE_PATH) {
            Ok(pipe) => return Ok(pipe),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("Failed to open named pipe"))
        .into())
}
