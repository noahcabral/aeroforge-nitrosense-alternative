use tauri::State;

use super::{
    blue_light,
    models::{
        ApplyState, BackendBootstrap, BackendContract, BlueLightApplyResult, CapabilitySnapshot,
        ControlSnapshot, FanCurveSet, FanProfileId, GpuTuningState, LiveControlSnapshot,
        PersistenceStatus, SmartChargeApplyResult,
        PowerProfileId, ProcessorStateSettings, ServiceStatus, ShellStatus, TelemetrySnapshot,
        UpdateChannelId, UpdateStatus,
    },
    service_pipe,
    smart_charge,
    state::{shell_status, BackendState},
    updater,
};

#[tauri::command]
pub fn runtime_shell() -> ShellStatus {
    shell_status()
}

#[tauri::command]
pub fn get_backend_contract(state: State<'_, BackendState>) -> BackendContract {
    state.contract()
}

#[tauri::command]
pub fn get_service_status(_state: State<'_, BackendState>) -> ServiceStatus {
    match service_pipe::fetch_service_status() {
        Ok(mut status) => {
            status.connected = true;
            status
        }
        Err(error) => service_pipe::fetch_cached_service_status(&error.to_string()),
    }
}

#[tauri::command]
pub fn get_capability_snapshot(state: State<'_, BackendState>) -> CapabilitySnapshot {
    service_pipe::fetch_capabilities().unwrap_or_else(|_| state.capabilities())
}

#[tauri::command]
pub fn get_control_snapshot(state: State<'_, BackendState>) -> ControlSnapshot {
    state.controls()
}

#[tauri::command]
pub fn get_telemetry_snapshot(state: State<'_, BackendState>) -> TelemetrySnapshot {
    service_pipe::fetch_telemetry()
        .or_else(|_| service_pipe::fetch_cached_telemetry())
        .unwrap_or_else(|_| state.telemetry())
}

#[tauri::command]
pub fn get_live_control_snapshot(
    _state: State<'_, BackendState>,
) -> Result<LiveControlSnapshot, String> {
    service_pipe::fetch_live_controls().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_backend_bootstrap(state: State<'_, BackendState>) -> BackendBootstrap {
    BackendBootstrap {
        shell: shell_status(),
        service: get_service_status(state.clone()),
        contract: state.contract(),
        capabilities: get_capability_snapshot(state.clone()),
        controls: state.controls(),
        telemetry: get_telemetry_snapshot(state),
    }
}

#[tauri::command]
pub fn get_persistence_status(state: State<'_, BackendState>) -> PersistenceStatus {
    state.persistence_status()
}

#[tauri::command]
pub fn get_update_status(state: State<'_, BackendState>) -> UpdateStatus {
    state.update_status()
}

#[tauri::command]
pub fn check_for_updates(
    channel: Option<UpdateChannelId>,
    state: State<'_, BackendState>,
) -> Result<UpdateStatus, String> {
    let resolved_channel = channel.unwrap_or_else(|| state.controls().personal_settings.update_channel);
    updater::refresh_status(state.updater(), resolved_channel).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stage_update_download(
    channel: Option<UpdateChannelId>,
    state: State<'_, BackendState>,
) -> Result<UpdateStatus, String> {
    let resolved_channel = channel.unwrap_or_else(|| state.controls().personal_settings.update_channel);
    updater::stage_latest_update(state.updater(), resolved_channel).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn install_staged_update(state: State<'_, BackendState>) -> Result<UpdateStatus, String> {
    updater::launch_staged_install(state.updater()).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn apply_blue_light_filter(
    enabled: bool,
    state: State<'_, BackendState>,
) -> Result<BlueLightApplyResult, String> {
    let applied = blue_light::apply_blue_light_filter(enabled).map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.personal_settings.blue_light_filter_enabled = applied.enabled;

    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    Ok(BlueLightApplyResult {
        controls,
        applied_at_unix: applied.applied_at_unix,
        gain_id: applied.gain_id,
        detail: applied.detail,
    })
}

#[tauri::command]
pub async fn apply_smart_charging(
    enabled: bool,
    state: State<'_, BackendState>,
) -> Result<SmartChargeApplyResult, String> {
    let applied = smart_charge::apply_smart_charging(enabled)
        .await
        .map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.personal_settings.smart_charging_enabled = applied.enabled;

    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    Ok(SmartChargeApplyResult {
        controls,
        applied_at_unix: applied.applied_at_unix,
        battery_healthy: applied.battery_healthy,
        detail: applied.detail,
    })
}

#[tauri::command]
pub fn save_control_snapshot(
    snapshot: ControlSnapshot,
    state: State<'_, BackendState>,
) -> Result<ControlSnapshot, String> {
    state
        .save_controls(snapshot)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reset_control_snapshot(state: State<'_, BackendState>) -> Result<ControlSnapshot, String> {
    state.reset_controls().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn apply_power_profile(
    profile_id: PowerProfileId,
    processor_state: ProcessorStateSettings,
    state: State<'_, BackendState>,
) -> Result<ControlSnapshot, String> {
    let applied = service_pipe::apply_power_profile(profile_id.clone(), processor_state.clone())
        .map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.active_power_profile = applied.profile_id;
    if matches!(controls.active_power_profile, PowerProfileId::Custom) {
        controls.custom_processor_state = applied.processor_state;
    }

    state
        .save_controls(controls)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn apply_gpu_tuning(
    tuning: GpuTuningState,
    active_oc_slot: String,
    state: State<'_, BackendState>,
) -> Result<super::models::GpuTuningApplyResult, String> {
    let applied = service_pipe::apply_gpu_tuning(tuning).map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.active_power_profile = PowerProfileId::Custom;
    controls.gpu_tuning = applied.tuning.clone();
    controls.active_oc_slot = active_oc_slot;
    controls.oc_apply_state = ApplyState::Live;

    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    Ok(super::models::GpuTuningApplyResult {
        controls,
        applied_at_unix: applied.applied_at_unix,
        detail: applied.detail,
    })
}

#[tauri::command]
pub fn apply_fan_profile(
    profile_id: FanProfileId,
    state: State<'_, BackendState>,
) -> Result<super::models::FanControlApplyResult, String> {
    let applied = service_pipe::apply_fan_profile(profile_id).map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.active_fan_profile = applied.profile_id;

    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    Ok(super::models::FanControlApplyResult {
        controls,
        applied_at_unix: applied.applied_at_unix,
        detail: applied.detail,
    })
}

#[tauri::command]
pub fn apply_custom_fan_curves(
    curves: FanCurveSet,
    state: State<'_, BackendState>,
) -> Result<super::models::FanControlApplyResult, String> {
    let applied =
        service_pipe::apply_custom_fan_curves(curves.clone()).map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.active_fan_profile = FanProfileId::Custom;
    controls.fan_curves = applied.curves.unwrap_or(curves);

    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    Ok(super::models::FanControlApplyResult {
        controls,
        applied_at_unix: applied.applied_at_unix,
        detail: applied.detail,
    })
}
