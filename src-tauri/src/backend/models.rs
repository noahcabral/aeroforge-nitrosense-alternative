use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellStatus {
    pub shell: String,
    pub backend_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceWorkerStatus {
    pub name: String,
    pub state: String,
    pub interval_seconds: u64,
    pub last_update_unix: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub connected: bool,
    pub pipe_name: String,
    pub service_name: String,
    pub version: Option<String>,
    pub state_dir: Option<String>,
    pub supervisor_file: Option<String>,
    pub worker_count: usize,
    pub updated_at_unix: Option<u64>,
    pub workers: Vec<ServiceWorkerStatus>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDescriptor {
    pub command: String,
    pub stage: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendContract {
    pub schema_version: String,
    pub commands: Vec<CommandDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureSupport {
    pub available: bool,
    pub writable: bool,
    pub requires_elevation: bool,
}

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
#[serde(rename_all = "kebab-case")]
pub enum PowerProfileId {
    BatteryGuard,
    Balanced,
    #[serde(alias = "performance")]
    Turbo,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FanProfileId {
    Auto,
    Max,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BootArtId {
    Ember,
    Arc,
    Slate,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateChannelId {
    Stable,
    Preview,
}

impl UpdateChannelId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Preview => "preview",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyState {
    Staged,
    Live,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTuningState {
    pub core_clock_mhz: i16,
    pub memory_clock_mhz: i16,
    pub voltage_offset_mv: i16,
    pub power_limit_percent: u8,
    pub temp_limit_c: u8,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcPreset {
    pub id: String,
    pub label: String,
    pub name: String,
    pub strap: String,
    pub settings: GpuTuningState,
    pub is_custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalSettings {
    #[serde(default = "default_true")]
    pub smart_charging_enabled: bool,
    #[serde(default = "default_true")]
    pub usb_power_enabled: bool,
    #[serde(default)]
    pub blue_light_filter_enabled: bool,
    #[serde(default = "default_boot_art")]
    pub selected_boot_art: BootArtId,
    #[serde(default = "default_custom_boot_filename")]
    pub custom_boot_filename: String,
    #[serde(default = "default_update_channel")]
    pub update_channel: UpdateChannelId,
    #[serde(default = "default_true")]
    pub check_for_updates_on_launch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlSnapshot {
    pub active_power_profile: PowerProfileId,
    pub active_fan_profile: FanProfileId,
    pub custom_processor_state: ProcessorStateSettings,
    pub gpu_tuning: GpuTuningState,
    pub oc_presets: Vec<OcPreset>,
    pub active_oc_slot: String,
    pub oc_apply_state: ApplyState,
    pub oc_tuning_locked: bool,
    pub fan_curves: FanCurveSet,
    #[serde(default)]
    pub fan_sync_lock_enabled: bool,
    pub personal_settings: PersonalSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveControlSnapshot {
    pub service: String,
    #[serde(default = "default_true")]
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
    #[serde(default = "default_waiting_power_apply_detail")]
    pub last_apply_detail: String,
    #[serde(default)]
    pub last_error: Option<String>,
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
    pub last_fan_readback: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

fn default_boot_art() -> BootArtId {
    BootArtId::Ember
}

fn default_custom_boot_filename() -> String {
    "custom-boot.png".into()
}

fn default_update_channel() -> UpdateChannelId {
    UpdateChannelId::Stable
}

fn default_waiting_power_apply_detail() -> String {
    "Waiting for the first control action.".into()
}

fn default_waiting_fan_apply_detail() -> String {
    "Waiting for the first fan-control apply.".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    pub cpu_temp_c: u8,
    pub cpu_temp_average_c: Option<u8>,
    pub cpu_temp_lowest_core_c: Option<u8>,
    pub cpu_temp_highest_core_c: Option<u8>,
    pub gpu_temp_c: u8,
    pub system_temp_c: u8,
    pub cpu_usage_percent: u8,
    pub gpu_usage_percent: u8,
    pub gpu_memory_usage_percent: Option<u8>,
    pub cpu_name: Option<String>,
    pub cpu_brand: Option<String>,
    pub gpu_name: Option<String>,
    pub gpu_brand: Option<String>,
    pub system_vendor: Option<String>,
    pub system_model: Option<String>,
    pub cpu_clock_mhz: u16,
    pub gpu_clock_mhz: u16,
    pub cpu_fan_rpm: u16,
    pub gpu_fan_rpm: u16,
    pub battery_percent: u8,
    pub battery_life_remaining_sec: Option<u32>,
    pub ac_plugged_in: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTuningApplyResult {
    pub controls: ControlSnapshot,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanControlApplyResult {
    pub controls: ControlSnapshot,
    pub applied_at_unix: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendBootstrap {
    pub shell: ShellStatus,
    pub service: ServiceStatus,
    pub contract: BackendContract,
    pub capabilities: CapabilitySnapshot,
    pub controls: ControlSnapshot,
    pub telemetry: TelemetrySnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceStatus {
    pub config_file: String,
    pub initialized_from_disk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    #[serde(default = "default_update_repo_slug")]
    pub repo_slug: String,
    #[serde(default = "default_current_version")]
    pub current_version: String,
    #[serde(default)]
    pub token_configured: bool,
    #[serde(default)]
    pub last_checked_at_unix: Option<u64>,
    #[serde(default)]
    pub update_available: bool,
    #[serde(default)]
    pub can_stage_update: bool,
    #[serde(default)]
    pub can_install_update: bool,
    #[serde(default = "default_update_feed_kind")]
    pub feed_kind: String,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub latest_title: Option<String>,
    #[serde(default)]
    pub latest_published_at: Option<String>,
    #[serde(default)]
    pub latest_commit_sha: Option<String>,
    #[serde(default)]
    pub latest_asset_name: Option<String>,
    #[serde(default)]
    pub staged_asset_name: Option<String>,
    #[serde(default)]
    pub staged_asset_path: Option<String>,
    #[serde(default)]
    pub staged_sha256: Option<String>,
    #[serde(default)]
    pub staged_at_unix: Option<u64>,
    #[serde(default = "default_update_detail")]
    pub detail: String,
    #[serde(default)]
    pub last_error: Option<String>,
}

fn default_update_repo_slug() -> String {
    "noahcabral/aeroforge-nitrosense-alternative".into()
}

fn default_current_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

fn default_update_feed_kind() -> String {
    "none".into()
}

fn default_update_detail() -> String {
    "Updater not checked yet.".into()
}
