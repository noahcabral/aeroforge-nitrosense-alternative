use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

use crate::models::*;

const PIPE_PATH: &str = r"\\.\pipe\AeroForgeService";
const PIPE_RETRY_ATTEMPTS: usize = 20;
const PIPE_RETRY_DELAY_MS: u64 = 100;

#[derive(Debug)]
pub enum ServiceClientError {
    PipeUnavailable(String),
    IoError(std::io::Error),
    SerializationError(serde_json::Error),
    ServiceError(String),
    EmptyResponse,
}

impl std::fmt::Display for ServiceClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PipeUnavailable(msg) => write!(f, "Service pipe unavailable: {}", msg),
            Self::IoError(err) => write!(f, "I/O error: {}", err),
            Self::SerializationError(err) => write!(f, "Serialization error: {}", err),
            Self::ServiceError(msg) => write!(f, "Service error: {}", msg),
            Self::EmptyResponse => write!(f, "Service returned empty response"),
        }
    }
}

impl std::error::Error for ServiceClientError {}

impl From<std::io::Error> for ServiceClientError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

impl From<serde_json::Error> for ServiceClientError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError(err)
    }
}

pub struct ServiceClient;

impl ServiceClient {
    /// Execute a request against the AeroForge service pipe
    pub fn request(command: PipeRequest) -> Result<serde_json::Value, ServiceClientError> {
        let mut pipe = Self::open_pipe_with_retry()?;

        let serialized = serde_json::to_string(&command)?;
        pipe.write_all(serialized.as_bytes())?;
        pipe.write_all(b"\n")?;
        pipe.flush()?;

        let mut reader = BufReader::new(pipe);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        if line.trim().is_empty() {
            return Err(ServiceClientError::EmptyResponse);
        }

        match serde_json::from_str::<PipeResponse>(&line)? {
            PipeResponse::Ok { payload } => Ok(payload),
            PipeResponse::Error { message } => Err(ServiceClientError::ServiceError(message)),
        }
    }

    /// Get service status
    pub fn get_service_status() -> Result<ServiceStatus, ServiceClientError> {
        let payload = Self::request(PipeRequest::GetServiceStatus)?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Get hardware capabilities
    pub fn get_capabilities() -> Result<CapabilitySnapshot, ServiceClientError> {
        let payload = Self::request(PipeRequest::GetCapabilities)?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Get current telemetry snapshot
    pub fn get_telemetry() -> Result<TelemetrySnapshot, ServiceClientError> {
        let payload = Self::request(PipeRequest::GetTelemetrySnapshot)?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Get control snapshot
    pub fn get_control_snapshot() -> Result<ControlSnapshot, ServiceClientError> {
        let payload = Self::request(PipeRequest::GetControlSnapshot)?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Restore startup state
    pub fn restore_startup_state() -> Result<String, ServiceClientError> {
        let payload = Self::request(PipeRequest::RestoreStartupState)?;
        Ok(payload
            .get("detail")
            .and_then(|v| v.as_str())
            .unwrap_or("Restored saved AeroForge state.")
            .to_string())
    }

    /// Apply power profile
    pub fn apply_power_profile(
        profile_id: PowerProfileId,
        processor_state: ProcessorStateSettings,
        custom_base_profile: Option<CustomPowerBaseId>,
        processor_state_control_enabled: bool,
    ) -> Result<AppliedPowerProfilePayload, ServiceClientError> {
        let payload = Self::request(PipeRequest::ApplyPowerProfile {
            payload: ApplyPowerProfileRequest {
                profile_id,
                processor_state,
                custom_base_profile,
                processor_state_control_enabled,
            },
        })?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Apply GPU tuning
    pub fn apply_gpu_tuning(
        tuning: GpuTuningState,
    ) -> Result<AppliedGpuTuningPayload, ServiceClientError> {
        let payload = Self::request(PipeRequest::ApplyGpuTuning {
            payload: ApplyGpuTuningRequest { tuning },
        })?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Apply fan profile
    pub fn apply_fan_profile(
        profile_id: FanProfileId,
    ) -> Result<AppliedFanControlPayload, ServiceClientError> {
        let payload = Self::request(PipeRequest::ApplyFanProfile {
            payload: ApplyFanProfileRequest { profile_id },
        })?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Apply custom fan curves
    pub fn apply_custom_fan_curves(
        curves: FanCurveSet,
    ) -> Result<AppliedFanControlPayload, ServiceClientError> {
        let payload = Self::request(PipeRequest::ApplyCustomFanCurves {
            payload: ApplyCustomFanCurvesRequest {
                curves,
                quiet_success_log: false,
            },
        })?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Start fan speed calibration
    pub fn start_fan_speed_calibration(
    ) -> Result<FanSpeedCalibrationSnapshot, ServiceClientError> {
        let payload = Self::request(PipeRequest::StartFanSpeedCalibration)?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Cancel fan speed calibration
    pub fn cancel_fan_speed_calibration(
    ) -> Result<FanSpeedCalibrationSnapshot, ServiceClientError> {
        let payload = Self::request(PipeRequest::CancelFanSpeedCalibration)?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Apply boot logo
    pub fn apply_boot_logo(
        image_path: String,
        original_filename: Option<String>,
    ) -> Result<AppliedBootLogoPayload, ServiceClientError> {
        let payload = Self::request(PipeRequest::ApplyBootLogo {
            payload: ApplyBootLogoRequest {
                image_path,
                original_filename,
            },
        })?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Apply smart charging
    pub fn apply_smart_charging(
        enabled: bool,
    ) -> Result<AppliedSmartChargePayload, ServiceClientError> {
        let payload = Self::request(PipeRequest::ApplySmartCharging {
            payload: ApplySmartChargeRequest { enabled },
        })?;
        Ok(serde_json::from_value(payload)?)
    }

    /// Apply telemetry settings
    pub fn apply_telemetry_settings(
        nvidia_telemetry_enabled: bool,
    ) -> Result<AppliedTelemetrySettingsPayload, ServiceClientError> {
        let payload = Self::request(PipeRequest::ApplyTelemetrySettings {
            payload: ApplyTelemetrySettingsRequest {
                nvidia_telemetry_enabled,
            },
        })?;
        Ok(serde_json::from_value(payload)?)
    }

    // ========================================================================
    // Cached Snapshot Helpers
    // ========================================================================

    /// Read cached telemetry from disk (fast polling)
    pub fn get_cached_telemetry() -> Result<TelemetrySnapshot, ServiceClientError> {
        let path = Self::telemetry_cache_path();
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Read cached control snapshot from disk
    pub fn get_cached_control_snapshot() -> Result<ControlSnapshot, ServiceClientError> {
        let path = Self::control_cache_path();
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Read cached capabilities from disk
    pub fn get_cached_capabilities() -> Result<CapabilitySnapshot, ServiceClientError> {
        let path = Self::capabilities_cache_path();
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Read cached service status (for fast polling without pipe connection)
    pub fn get_cached_service_status() -> Result<ServiceStatus, ServiceClientError> {
        let supervisor_path = Self::supervisor_cache_path();
        let raw = fs::read_to_string(supervisor_path)?;

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SupervisorSnapshot {
            service: String,
            worker_count: usize,
            updated_at_unix: Option<u64>,
            workers: Vec<ServiceWorkerStatus>,
        }

        let snapshot: SupervisorSnapshot = serde_json::from_str(&raw)?;

        Ok(ServiceStatus {
            connected: true,
            pipe_name: PIPE_PATH.to_string(),
            service_name: snapshot.service,
            version: None,
            state_dir: Some(Self::state_dir().display().to_string()),
            supervisor_file: Some(Self::supervisor_cache_path().display().to_string()),
            worker_count: snapshot.worker_count,
            updated_at_unix: snapshot.updated_at_unix,
            workers: snapshot.workers,
            detail: "Loaded from cached supervisor snapshot.".to_string(),
        })
    }

    // ========================================================================
    // Internal Helpers
    // ========================================================================

    fn open_pipe_with_retry() -> Result<std::fs::File, ServiceClientError> {
        let mut last_error: Option<std::io::Error> = None;

        for _attempt in 0..PIPE_RETRY_ATTEMPTS {
            match OpenOptions::new().read(true).write(true).open(PIPE_PATH) {
                Ok(pipe) => return Ok(pipe),
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(PIPE_RETRY_DELAY_MS));
                }
            }
        }

        Err(ServiceClientError::PipeUnavailable(
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Unknown pipe error".to_string()),
        ))
    }

    fn state_dir() -> PathBuf {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join("AeroForge")
            .join("Service")
            .join("state")
    }

    fn telemetry_cache_path() -> PathBuf {
        Self::state_dir().join("telemetry.json")
    }

    fn control_cache_path() -> PathBuf {
        Self::state_dir().join("control.json")
    }

    fn capabilities_cache_path() -> PathBuf {
        Self::state_dir().join("capabilities.json")
    }

    fn supervisor_cache_path() -> PathBuf {
        Self::state_dir().join("supervisor.json")
    }
}
