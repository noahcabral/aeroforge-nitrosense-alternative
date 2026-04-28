use std::{
    env,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::models::{
    CapabilitySnapshot, CustomPowerBaseId, FanCurveSet, FanProfileId, GpuTuningState,
    LiveControlSnapshot, PowerProfileId, ProcessorStateSettings, ServiceStatus,
    ServiceWorkerStatus, TelemetrySnapshot,
};

const PIPE_PATH: &str = r"\\.\pipe\AeroForgeService";
const SERVICE_NAME: &str = "AeroForgeService";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedSupervisorSnapshot {
    service: String,
    worker_count: usize,
    updated_at_unix: Option<u64>,
    workers: Vec<ServiceWorkerStatus>,
}

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
    ApplyBootLogo {
        payload: ApplyBootLogoRequest,
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
    custom_base_profile: Option<CustomPowerBaseId>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyBootLogoRequest {
    image_path: String,
    original_filename: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPowerProfilePayload {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedBootLogoPayload {
    pub applied_at_unix: u64,
    pub detail: String,
}

pub fn fetch_cached_service_status(pipe_error: &str) -> ServiceStatus {
    let state_dir = service_state_dir();
    let supervisor_file = supervisor_file_path();
    let cached_snapshot = fs::read_to_string(&supervisor_file)
        .ok()
        .and_then(|raw| serde_json::from_str::<CachedSupervisorSnapshot>(&raw).ok());

    let (service_name, worker_count, updated_at_unix, workers) =
        if let Some(snapshot) = cached_snapshot {
            (
                snapshot.service,
                snapshot.worker_count,
                snapshot.updated_at_unix,
                snapshot.workers,
            )
        } else {
            (SERVICE_NAME.into(), 0, None, Vec::new())
        };

    let missing_pipe_detail = if pipe_error.contains("os error 2") {
        format!(
            "{SERVICE_NAME} is not installed or is not running. Install AeroForge with the setup installer, or start {SERVICE_NAME}. Raw pipe error: {pipe_error}"
        )
    } else {
        format!("Service unavailable: {pipe_error}")
    };

    let detail = if supervisor_file.exists() {
        format!(
            "{missing_pipe_detail}. Loaded cached supervisor snapshot from {}.",
            supervisor_file.display()
        )
    } else {
        missing_pipe_detail
    };

    ServiceStatus {
        connected: false,
        pipe_name: PIPE_PATH.into(),
        service_name,
        version: None,
        state_dir: Some(state_dir.display().to_string()),
        supervisor_file: Some(supervisor_file.display().to_string()),
        worker_count,
        updated_at_unix,
        workers,
        detail,
    }
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

pub fn fetch_cached_telemetry(
) -> Result<TelemetrySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let raw = fs::read_to_string(telemetry_file_path())?;
    Ok(serde_json::from_str::<TelemetrySnapshot>(&raw)?)
}

pub fn fetch_live_controls() -> Result<LiveControlSnapshot, Box<dyn std::error::Error + Send + Sync>>
{
    let payload = request(PipeRequest::GetControlSnapshot)?;
    Ok(serde_json::from_value::<LiveControlSnapshot>(payload)?)
}

pub fn apply_power_profile(
    profile_id: PowerProfileId,
    processor_state: ProcessorStateSettings,
    custom_base_profile: Option<CustomPowerBaseId>,
) -> Result<AppliedPowerProfilePayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyPowerProfile {
        payload: ApplyPowerProfileRequest {
            profile_id,
            processor_state,
            custom_base_profile,
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

pub fn apply_boot_logo(
    image_path: String,
    original_filename: Option<String>,
) -> Result<AppliedBootLogoPayload, Box<dyn std::error::Error + Send + Sync>> {
    let payload = request(PipeRequest::ApplyBootLogo {
        payload: ApplyBootLogoRequest {
            image_path,
            original_filename,
        },
    })?;
    Ok(serde_json::from_value::<AppliedBootLogoPayload>(payload)?)
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

fn service_state_dir() -> PathBuf {
    env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("AeroForge")
        .join("Service")
        .join("state")
}

fn telemetry_file_path() -> PathBuf {
    service_state_dir().join("telemetry.json")
}

fn supervisor_file_path() -> PathBuf {
    service_state_dir().join("supervisor.json")
}
