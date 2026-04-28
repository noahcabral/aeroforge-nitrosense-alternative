use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerProfileId {
    BatteryGuard,
    Balanced,
    #[serde(alias = "performance")]
    Performance,
    Turbo,
    Custom,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CustomPowerBaseId {
    Balanced,
    Performance,
    Turbo,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorStateSettings {
    pub min_percent: u8,
    pub max_percent: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorStateReadback {
    pub ac: ProcessorStateSettings,
    pub dc: ProcessorStateSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTuningState {
    pub core_clock_mhz: i16,
    pub memory_clock_mhz: i16,
    pub voltage_offset_mv: i16,
    pub power_limit_percent: u8,
    pub temp_limit_c: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FanProfileId {
    Auto,
    Max,
    Custom,
}

impl FanProfileId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Max => "max",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FanCurvePoint {
    pub temp_c: u8,
    pub speed_percent: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FanCurveSet {
    pub cpu: Vec<FanCurvePoint>,
    pub gpu: Vec<FanCurvePoint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPowerProfileRequest {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
    #[serde(default)]
    pub custom_base_profile: Option<CustomPowerBaseId>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyGpuTuningRequest {
    pub tuning: GpuTuningState,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyFanProfileRequest {
    pub profile_id: FanProfileId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCustomFanCurvesRequest {
    pub curves: FanCurveSet,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBootLogoRequest {
    pub image_path: String,
    #[serde(default)]
    pub original_filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPowerProfileSnapshot {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
    pub readback: ProcessorStateReadback,
    pub drift_detected: bool,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedGpuTuningSnapshot {
    pub tuning: GpuTuningState,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFanControlSnapshot {
    pub profile_id: FanProfileId,
    pub curves: Option<FanCurveSet>,
    pub cpu_speed_percent: Option<u8>,
    pub gpu_speed_percent: Option<u8>,
    pub readback: Option<Value>,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedBootLogoSnapshot {
    pub image_path: String,
    #[serde(default)]
    pub original_filename: Option<String>,
    pub readback: Option<Value>,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlSnapshot {
    pub service: String,
    pub power_apply_supported: bool,
    #[serde(default = "default_true")]
    pub gpu_tuning_apply_supported: bool,
    #[serde(default = "default_true")]
    pub fan_apply_supported: bool,
    #[serde(default = "default_true")]
    pub fan_curve_apply_supported: bool,
    pub active_power_profile: Option<PowerProfileId>,
    pub processor_state: Option<ProcessorStateSettings>,
    #[serde(default)]
    pub processor_state_readback: Option<ProcessorStateReadback>,
    #[serde(default)]
    pub processor_state_drift_detected: bool,
    pub last_applied_at_unix: Option<u64>,
    pub last_apply_detail: String,
    pub last_error: Option<String>,
    #[serde(default)]
    pub active_gpu_tuning: Option<GpuTuningState>,
    #[serde(default)]
    pub last_gpu_tuning_applied_at_unix: Option<u64>,
    #[serde(default)]
    pub last_gpu_tuning_detail: String,
    #[serde(default)]
    pub last_gpu_tuning_error: Option<String>,
    #[serde(default)]
    pub active_fan_profile: Option<FanProfileId>,
    #[serde(default)]
    pub active_fan_curves: Option<FanCurveSet>,
    #[serde(default)]
    pub last_fan_applied_at_unix: Option<u64>,
    #[serde(default = "default_waiting_fan_apply_detail")]
    pub last_fan_apply_detail: String,
    #[serde(default)]
    pub last_fan_error: Option<String>,
    #[serde(default)]
    pub last_fan_readback: Option<Value>,
    #[serde(default = "default_false")]
    pub boot_logo_apply_supported: bool,
    #[serde(default)]
    pub last_boot_logo_applied_at_unix: Option<u64>,
    #[serde(default = "default_waiting_boot_logo_apply_detail")]
    pub last_boot_logo_apply_detail: String,
    #[serde(default)]
    pub last_boot_logo_error: Option<String>,
    #[serde(default)]
    pub last_boot_logo_readback: Option<Value>,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_waiting_fan_apply_detail() -> String {
    "Waiting for the first fan-control apply.".into()
}

fn default_waiting_boot_logo_apply_detail() -> String {
    "Boot-logo firmware apply is disabled until a direct hardware path is implemented.".into()
}

impl ControlSnapshot {
    pub fn default_snapshot(service: &'static str) -> Self {
        Self {
            service: service.into(),
            power_apply_supported: true,
            gpu_tuning_apply_supported: true,
            fan_apply_supported: true,
            fan_curve_apply_supported: true,
            active_power_profile: None,
            processor_state: None,
            processor_state_readback: None,
            processor_state_drift_detected: false,
            last_applied_at_unix: None,
            last_apply_detail: "Waiting for the first control action.".into(),
            last_error: None,
            active_gpu_tuning: None,
            last_gpu_tuning_applied_at_unix: None,
            last_gpu_tuning_detail: "Waiting for the first GPU tuning apply.".into(),
            last_gpu_tuning_error: None,
            active_fan_profile: None,
            active_fan_curves: None,
            last_fan_applied_at_unix: None,
            last_fan_apply_detail: default_waiting_fan_apply_detail(),
            last_fan_error: None,
            last_fan_readback: None,
            boot_logo_apply_supported: false,
            last_boot_logo_applied_at_unix: None,
            last_boot_logo_apply_detail: default_waiting_boot_logo_apply_detail(),
            last_boot_logo_error: None,
            last_boot_logo_readback: None,
        }
    }
}
