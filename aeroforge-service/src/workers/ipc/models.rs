use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workers::control::{
    ApplyBootLogoRequest, ApplyCustomFanCurvesRequest, ApplyFanProfileRequest,
    ApplyGpuTuningRequest, ApplyPowerProfileRequest, ApplySmartChargeRequest,
    ApplyTelemetrySettingsRequest,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PipeRequest {
    GetServiceStatus,
    GetCapabilities,
    GetControlSnapshot,
    GetTelemetrySnapshot,
    RestoreStartupState {
        #[serde(default)]
        write_token: Option<String>,
    },
    ApplyPowerProfile {
        payload: ApplyPowerProfileRequest,
        #[serde(default)]
        write_token: Option<String>,
    },
    ApplyGpuTuning {
        payload: ApplyGpuTuningRequest,
        #[serde(default)]
        write_token: Option<String>,
    },
    ApplyFanProfile {
        payload: ApplyFanProfileRequest,
        #[serde(default)]
        write_token: Option<String>,
    },
    ApplyCustomFanCurves {
        payload: ApplyCustomFanCurvesRequest,
        #[serde(default)]
        write_token: Option<String>,
    },
    StartFanSpeedCalibration {
        #[serde(default)]
        write_token: Option<String>,
    },
    CancelFanSpeedCalibration {
        #[serde(default)]
        write_token: Option<String>,
    },
    ApplyBootLogo {
        payload: ApplyBootLogoRequest,
        #[serde(default)]
        write_token: Option<String>,
    },
    ApplySmartCharging {
        payload: ApplySmartChargeRequest,
        #[serde(default)]
        write_token: Option<String>,
    },
    ApplyTelemetrySettings {
        payload: ApplyTelemetrySettingsRequest,
        #[serde(default)]
        write_token: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PipeResponse {
    Ok { payload: Value },
    Error { message: String },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorSnapshot {
    pub service: String,
    pub worker_count: usize,
    pub updated_at_unix: u64,
    pub workers: Vec<WorkerStatusSnapshot>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerStatusSnapshot {
    pub name: String,
    pub state: String,
    pub interval_seconds: u64,
    pub last_update_unix: u64,
    pub last_error: Option<String>,
}
