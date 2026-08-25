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
    #[serde(default = "default_true")]
    pub processor_state_control_enabled: bool,
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
    #[serde(default)]
    pub quiet_success_log: bool,
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
pub struct ApplySmartChargeRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyTelemetrySettingsRequest {
    pub nvidia_telemetry_enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPowerProfileSnapshot {
    pub profile_id: PowerProfileId,
    pub processor_state: ProcessorStateSettings,
    #[serde(default)]
    pub custom_base_profile: Option<CustomPowerBaseId>,
    #[serde(default = "default_true")]
    pub processor_state_control_enabled: bool,
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFanControlSnapshot {
    pub profile_id: FanProfileId,
    pub curves: Option<FanCurveSet>,
    pub cpu_speed_percent: Option<u8>,
    pub gpu_speed_percent: Option<u8>,
    pub readback: Option<Value>,
    #[serde(default)]
    pub quiet_auto_fan_calibration: Option<QuietAutoFanCalibration>,
    #[serde(default)]
    pub quiet_auto_thermal_warning: Option<QuietAutoThermalWarning>,
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
pub struct AppliedSmartChargeSnapshot {
    pub enabled: bool,
    pub health_status: u8,
    pub battery_healthy: u8,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedTelemetrySettingsSnapshot {
    pub nvidia_telemetry_enabled: bool,
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
    pub custom_base_profile: Option<CustomPowerBaseId>,
    #[serde(default = "default_true")]
    pub processor_state_control_enabled: bool,
    #[serde(default = "default_true")]
    pub nvidia_telemetry_enabled: bool,
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
    pub current_cpu_fan_speed_percent: Option<u8>,
    #[serde(default)]
    pub current_gpu_fan_speed_percent: Option<u8>,
    #[serde(default)]
    pub last_fan_applied_at_unix: Option<u64>,
    #[serde(default = "default_waiting_fan_apply_detail")]
    pub last_fan_apply_detail: String,
    #[serde(default)]
    pub last_fan_error: Option<String>,
    #[serde(default)]
    pub last_fan_readback: Option<Value>,
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

fn default_waiting_fan_apply_detail() -> String {
    "Waiting for the first fan-control apply.".into()
}

fn default_waiting_boot_logo_apply_detail() -> String {
    "Boot-logo apply is ready. AeroForge will write only after EFI partition preflight, backup, and verification pass.".into()
}

impl ControlSnapshot {
    pub fn default_snapshot(service: &'static str) -> Self {
        Self {
            service: service.into(),
            power_apply_supported: true,
            gpu_tuning_apply_supported: true,
            fan_apply_supported: true,
            fan_curve_apply_supported: true,
            active_power_profile: Some(PowerProfileId::Turbo),
            processor_state: Some(ProcessorStateSettings {
                min_percent: 100,
                max_percent: 100,
            }),
            custom_base_profile: None,
            processor_state_control_enabled: true,
            nvidia_telemetry_enabled: true,
            processor_state_readback: None,
            processor_state_drift_detected: false,
            last_applied_at_unix: None,
            last_apply_detail: "Waiting for the first control action.".into(),
            last_error: None,
            active_gpu_tuning: None,
            last_gpu_tuning_applied_at_unix: None,
            last_gpu_tuning_detail: "Waiting for the first GPU tuning apply.".into(),
            last_gpu_tuning_error: None,
            active_fan_profile: Some(FanProfileId::Auto),
            active_fan_curves: None,
            current_cpu_fan_speed_percent: None,
            current_gpu_fan_speed_percent: None,
            last_fan_applied_at_unix: None,
            last_fan_apply_detail: default_waiting_fan_apply_detail(),
            last_fan_error: None,
            last_fan_readback: None,
            quiet_auto_fan_calibration: QuietAutoFanCalibration::default(),
            quiet_auto_thermal_warning: None,
            fan_speed_calibration: FanSpeedCalibrationSnapshot::default(),
            boot_logo_apply_supported: true,
            last_boot_logo_applied_at_unix: None,
            last_boot_logo_apply_detail: default_waiting_boot_logo_apply_detail(),
            last_boot_logo_error: None,
            last_boot_logo_readback: None,
        }
    }
}
