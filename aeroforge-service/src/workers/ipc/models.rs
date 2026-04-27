use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workers::control::{
    ApplyBootLogoRequest, ApplyCustomFanCurvesRequest, ApplyFanProfileRequest, ApplyGpuTuningRequest,
    ApplyPowerProfileRequest,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PipeRequest {
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
