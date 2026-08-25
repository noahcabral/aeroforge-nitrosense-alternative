use serde::{Deserialize, Serialize};

// ============================================================================
// Power Profile Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PowerProfileId {
    BatteryGuard,
    Balanced,
    #[serde(alias = "performance")]
    Performance,
    Turbo,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CustomPowerBaseId {
    Balanced,
    Performance,
    Turbo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorStateSettings {
    pub min_percent: u8,
    pub max_percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorStateReadback {
    pub ac: ProcessorStateSettings,
    pub dc: ProcessorStateSettings,
}

// ============================================================================
// Fan Profile Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FanProfileId {
    Auto,
    Max,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanCurvePoint {
    pub temp_c: u8,
    pub speed_percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanCurveSet {
    pub cpu: Vec<FanCurvePoint>,
    pub gpu: Vec<FanCurvePoint>,
}

// ============================================================================
// GPU Tuning Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTuningState {
    pub core_clock_mhz: i16,
    pub memory_clock_mhz: i16,
    pub voltage_offset_mv: i16,
    pub power_limit_percent: u8,
    pub temp_limit_c: u8,
}

// ============================================================================
// Telemetry Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    pub cpu_temp_c: u8,
    #[serde(default)]
    pub cpu_temp_average_c: Option<u8>,
    #[serde(default)]
    pub cpu_temp_lowest_core_c: Option<u8>,
    #[serde(default)]
    pub cpu_temp_highest_core_c: Option<u8>,
    pub gpu_temp_c: u8,
    pub system_temp_c: u8,
    pub cpu_usage_percent: u8,
    pub gpu_usage_percent: u8,
    #[serde(default)]
    pub gpu_memory_usage_percent: Option<u8>,
    #[serde(default)]
    pub gpu_power_draw_w: Option<f32>,
    #[serde(default)]
    pub gpu_power_limit_w: Option<f32>,
    #[serde(default)]
    pub gpu_power_default_limit_w: Option<f32>,
    #[serde(default)]
    pub gpu_power_min_limit_w: Option<f32>,
    #[serde(default)]
    pub gpu_power_max_limit_w: Option<f32>,
    #[serde(default)]
    pub cpu_package_power_w: Option<f32>,
    #[serde(default)]
    pub cpu_pl1_w: Option<f32>,
    #[serde(default)]
    pub cpu_pl1_enabled: Option<bool>,
    #[serde(default)]
    pub cpu_pl2_w: Option<f32>,
    #[serde(default)]
    pub cpu_pl2_enabled: Option<bool>,
    #[serde(default)]
    pub cpu_power_limit_locked: Option<bool>,
    #[serde(default)]
    pub cpu_name: Option<String>,
    #[serde(default)]
    pub cpu_brand: Option<String>,
    #[serde(default)]
    pub gpu_name: Option<String>,
    #[serde(default)]
    pub gpu_brand: Option<String>,
    #[serde(default)]
    pub system_vendor: Option<String>,
    #[serde(default)]
    pub system_model: Option<String>,
    pub cpu_clock_mhz: u16,
    pub gpu_clock_mhz: u16,
    pub cpu_fan_rpm: u16,
    pub gpu_fan_rpm: u16,
    pub battery_percent: u8,
    #[serde(default)]
    pub battery_life_remaining_sec: Option<u32>,
    pub ac_plugged_in: bool,
    #[serde(default)]
    pub heartbeat: u64,
}

// ============================================================================
// Service Status Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub connected: bool,
    pub pipe_name: String,
    pub service_name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub state_dir: Option<String>,
    #[serde(default)]
    pub supervisor_file: Option<String>,
    pub worker_count: usize,
    #[serde(default)]
    pub updated_at_unix: Option<u64>,
    pub workers: Vec<ServiceWorkerStatus>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceWorkerStatus {
    pub name: String,
    pub state: String,
    pub interval_seconds: u64,
    pub last_update_unix: u64,
    #[serde(default)]
    pub last_error: Option<String>,
}

// ============================================================================
// Capability Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshot {
    pub power_profiles: FeatureSupport,
    pub fan_profiles: FeatureSupport,
    pub fan_curves: FeatureSupport,
    pub smart_charging: FeatureSupport,
    pub usb_power: FeatureSupport,
    pub blue_light_filter: FeatureSupport,
    pub gpu_tuning: FeatureSupport,
    pub boot_logo: FeatureSupport,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureSupport {
    pub available: bool,
    pub writable: bool,
    pub requires_elevation: bool,
}

// ============================================================================
// Control Snapshot Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlSnapshot {
    pub service: String,
    #[serde(default = "default_true")]
    pub power_apply_supported: bool,
    #[serde(default = "default_true")]
    pub gpu_tuning_apply_supported: bool,
    #[serde(default = "default_true")]
    pub fan_apply_supported: bool,
    #[serde(default = "default_true")]
    pub fan_curve_apply_supported: bool,
    #[serde(default)]
    pub active_power_profile: Option<PowerProfileId>,
    #[serde(default)]
    pub processor_state: Option<ProcessorStateSettings>,
    #[serde(default)]
    pub custom_base_profile: Option<CustomPowerBaseId>,
    #[serde(default = "default_true")]
    pub processor_state_control_enabled: bool,
    #[serde(default = "default_true")]
    pub nvidia_telemetry_enabled: bool,
    #[serde(default)]
    pub processor_state_readback: Option<ProcessorStateReadback>,
    #[serde(default)]
    pub processor_state_drift_detected: bool,
    #[serde(default)]
    pub last_applied_at_unix: Option<u64>,
    pub last_apply_detail: String,
    #[serde(default)]
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
    pub current_cpu_fan_speed_percent: Option<u8>,
    #[serde(default)]
    pub current_gpu_fan_speed_percent: Option<u8>,
    #[serde(default)]
    pub last_fan_applied_at_unix: Option<u64>,
    pub last_fan_apply_detail: String,
    #[serde(default)]
    pub last_fan_error: Option<String>,
    #[serde(default)]
    pub last_fan_readback: Option<serde_json::Value>,
    #[serde(default)]
    pub quiet_auto_fan_calibration: QuietAutoFanCalibration,
    #[serde(default)]
    pub quiet_auto_thermal_warning: Option<QuietAutoThermalWarning>,
    #[serde(default)]
    pub fan_speed_calibration: FanSpeedCalibrationSnapshot,
    #[serde(default = "default_true")]
    pub boot_logo_apply_supported: bool,
    #[serde(default)]
    pub last_boot_logo_applied_at_unix: Option<u64>,
    pub last_boot_logo_apply_detail: String,
    #[serde(default)]
    pub last_boot_logo_error: Option<String>,
    #[serde(default)]
    pub last_boot_logo_readback: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietAutoFanMap {
    #[serde(default)]
    pub last_percent: Option<u8>,
    #[serde(default)]
    pub last_rpm: Option<u16>,
    #[serde(default)]
    pub idle_percent: Option<u8>,
    #[serde(default)]
    pub elevated_percent: Option<u8>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietAutoFanCalibration {
    #[serde(default)]
    pub cpu: QuietAutoFanMap,
    #[serde(default)]
    pub gpu: QuietAutoFanMap,
    #[serde(default)]
    pub last_target_rpm: Option<u16>,
    #[serde(default)]
    pub updated_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietAutoThermalWarning {
    pub active: bool,
    #[serde(default)]
    pub sensor: Option<String>,
    #[serde(default)]
    pub temp_c: Option<u8>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub updated_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanSpeedCalibrationPoint {
    pub percent: u8,
    #[serde(default)]
    pub cpu_rpm: Option<u16>,
    #[serde(default)]
    pub gpu_rpm: Option<u16>,
    #[serde(default)]
    pub cpu_temp_c: Option<u16>,
    #[serde(default)]
    pub gpu_temp_c: Option<u16>,
    #[serde(default)]
    pub system_temp_c: Option<u16>,
    pub sampled_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanSpeedCalibrationSnapshot {
    pub running: bool,
    pub status: String,
    #[serde(default)]
    pub started_at_unix: Option<u64>,
    #[serde(default)]
    pub updated_at_unix: Option<u64>,
    #[serde(default)]
    pub completed_at_unix: Option<u64>,
    #[serde(default)]
    pub current_percent: Option<u8>,
    pub settle_seconds: u64,
    #[serde(default)]
    pub points: Vec<FanSpeedCalibrationPoint>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl Default for FanSpeedCalibrationSnapshot {
    fn default() -> Self {
        Self {
            running: false,
            status: "Fan speed calibration has not been run.".into(),
            started_at_unix: None,
            updated_at_unix: None,
            completed_at_unix: None,
            current_percent: None,
            settle_seconds: 20,
            points: Vec::new(),
            last_error: None,
        }
    }
}

// ============================================================================
// IPC Request/Response Models
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PipeRequest {
    GetServiceStatus,
    GetCapabilities,
    GetControlSnapshot,
    GetTelemetrySnapshot,
    RestoreStartupState,
    ApplyPowerProfile { payload: ApplyPowerProfileRequest },
    ApplyGpuTuning { payload: ApplyGpuTuningRequest },
    ApplyFanProfile { payload: ApplyFanProfileRequest },
    ApplyCustomFanCurves { payload: ApplyCustomFanCurvesRequest },
    StartFanSpeedCalibration,
    CancelFanSpeedCalibration,
    ApplyBootLogo { payload: ApplyBootLogoRequest },
    ApplySmartCharging { payload: ApplySmartChargeRequest },
    ApplyTelemetrySettings { payload: ApplyTelemetrySettingsRequest },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PipeResponse {
    Ok { payload: serde_json::Value },
    Error { message: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPowerProfileRequest {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_base_profile: Option<CustomPowerBaseId>,
    pub processor_state_control_enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyGpuTuningRequest {
    pub tuning: GpuTuningState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyFanProfileRequest {
    pub profile_id: FanProfileId,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCustomFanCurvesRequest {
    pub curves: FanCurveSet,
    #[serde(skip_serializing_if = "is_false")]
    pub quiet_success_log: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyBootLogoRequest {
    pub image_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_filename: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySmartChargeRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyTelemetrySettingsRequest {
    pub nvidia_telemetry_enabled: bool,
}

// ============================================================================
// Response Payload Models
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPowerProfilePayload {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
    #[serde(default)]
    pub custom_base_profile: Option<CustomPowerBaseId>,
    pub processor_state_control_enabled: bool,
    pub readback: ProcessorStateReadback,
    pub drift_detected: bool,
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
    #[serde(default)]
    pub curves: Option<FanCurveSet>,
    #[serde(default)]
    pub cpu_speed_percent: Option<u8>,
    #[serde(default)]
    pub gpu_speed_percent: Option<u8>,
    #[serde(default)]
    pub readback: Option<serde_json::Value>,
    #[serde(default)]
    pub quiet_auto_fan_calibration: Option<QuietAutoFanCalibration>,
    #[serde(default)]
    pub quiet_auto_thermal_warning: Option<QuietAutoThermalWarning>,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedBootLogoPayload {
    pub image_path: String,
    #[serde(default)]
    pub original_filename: Option<String>,
    #[serde(default)]
    pub readback: Option<serde_json::Value>,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedSmartChargePayload {
    pub enabled: bool,
    pub health_status: u8,
    pub battery_healthy: u8,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedTelemetrySettingsPayload {
    pub nvidia_telemetry_enabled: bool,
    pub detail: String,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !*value
}
