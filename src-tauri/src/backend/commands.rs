use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};

use tauri::State;

use super::{
    blue_light, boot_logo, cpu_clock, display_refresh,
    models::{
        ApplyState, BackendBootstrap, BackendContract, BackendPollSnapshot, BackendPollTimings,
        BlueLightApplyResult, BootLogoApplyResult, CapabilitySnapshot, ControlSnapshot,
        CustomPowerBaseId, FanCurveSet, FanProfileId, GpuTuningState, LiveControlSnapshot,
        NvidiaTelemetryApplyResult, PerformanceLogEvent, PersistenceStatus, PowerProfileId,
        ProcessorStateSettings, ServiceStatus, ShellStatus, SmartChargeApplyResult,
        TelemetrySnapshot, UpdateChannelId, UpdateStatus,
    },
    service_pipe, smart_charge,
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
        Ok(status) => status,
        Err(error) => service_pipe::fetch_cached_service_status(&error.to_string()),
    }
}

#[tauri::command]
pub fn get_capability_snapshot(state: State<'_, BackendState>) -> CapabilitySnapshot {
    let desktop = state.capabilities();
    service_pipe::fetch_cached_capabilities()
        .or_else(|_| service_pipe::fetch_capabilities())
        .map(|service| merge_capabilities(desktop.clone(), service))
        .unwrap_or(desktop)
}

#[tauri::command]
pub fn get_control_snapshot(state: State<'_, BackendState>) -> ControlSnapshot {
    state.controls()
}

#[tauri::command]
pub fn get_telemetry_snapshot(state: State<'_, BackendState>) -> TelemetrySnapshot {
    let mut telemetry = service_pipe::fetch_cached_telemetry()
        .or_else(|_| service_pipe::fetch_telemetry())
        .unwrap_or_else(|_| state.telemetry());

    refresh_cpu_clock_if_missing(&mut telemetry);

    telemetry
}

#[tauri::command]
pub fn get_backend_poll_snapshot(state: State<'_, BackendState>) -> BackendPollSnapshot {
    let total_started = Instant::now();

    let service_started = Instant::now();
    let service = service_pipe::fetch_fast_service_status();
    let service_ms = elapsed_ms(service_started);

    let telemetry_started = Instant::now();
    let mut telemetry =
        service_pipe::fetch_cached_telemetry().unwrap_or_else(|_| state.telemetry());
    refresh_cpu_clock_if_missing(&mut telemetry);
    let telemetry_ms = elapsed_ms(telemetry_started);

    let live_controls_started = Instant::now();
    let live_controls = if service.connected {
        service_pipe::fetch_cached_live_controls().ok()
    } else {
        None
    };
    let live_controls_ms = elapsed_ms(live_controls_started);

    BackendPollSnapshot {
        service,
        telemetry,
        live_controls,
        timings: BackendPollTimings {
            total_ms: elapsed_ms(total_started),
            service_ms,
            telemetry_ms,
            live_controls_ms,
        },
    }
}

fn refresh_cpu_clock_if_missing(telemetry: &mut TelemetrySnapshot) {
    if telemetry.cpu_clock_mhz == 0 {
        if let Some(cpu_clock_mhz) = cpu_clock::read_effective_cpu_clock_mhz() {
            telemetry.cpu_clock_mhz = cpu_clock_mhz;
        }
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[tauri::command]
pub fn get_live_control_snapshot(
    _state: State<'_, BackendState>,
) -> Result<LiveControlSnapshot, String> {
    service_pipe::fetch_live_controls().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_backend_bootstrap(state: State<'_, BackendState>) -> BackendBootstrap {
    let poll = get_backend_poll_snapshot(state.clone());

    BackendBootstrap {
        shell: shell_status(),
        service: poll.service,
        contract: state.contract(),
        capabilities: get_capability_snapshot(state.clone()),
        controls: state.controls(),
        telemetry: poll.telemetry,
        live_controls: poll.live_controls,
        persistence: state.persistence_status(),
        update_status: state.update_status(),
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
pub fn append_performance_log(
    events: Vec<PerformanceLogEvent>,
    state: State<'_, BackendState>,
) -> Result<String, String> {
    let path = performance_log_path(&state).map_err(|error| error.to_string())?;

    if events.is_empty() {
        return Ok(path.display().to_string());
    }

    rotate_performance_log_if_needed(&path).map_err(|error| error.to_string())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| error.to_string())?;

    for event in events.into_iter().take(128) {
        serde_json::to_writer(&mut file, &event).map_err(|error| error.to_string())?;
        writeln!(file).map_err(|error| error.to_string())?;
    }

    Ok(path.display().to_string())
}

#[tauri::command]
pub fn check_for_updates(
    channel: Option<UpdateChannelId>,
    state: State<'_, BackendState>,
) -> Result<UpdateStatus, String> {
    let resolved_channel =
        channel.unwrap_or_else(|| state.controls().personal_settings.update_channel);
    updater::refresh_status(state.updater(), resolved_channel).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stage_update_download(
    channel: Option<UpdateChannelId>,
    state: State<'_, BackendState>,
) -> Result<UpdateStatus, String> {
    let resolved_channel =
        channel.unwrap_or_else(|| state.controls().personal_settings.update_channel);
    updater::stage_latest_update(state.updater(), resolved_channel)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn install_staged_update(state: State<'_, BackendState>) -> Result<UpdateStatus, String> {
    updater::launch_staged_install(state.updater()).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn show_update_notification(version_label: String) -> Result<(), String> {
    show_desktop_update_notification(&version_label)
}

#[cfg(windows)]
fn show_desktop_update_notification(version_label: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let version = normalize_notification_text(version_label, 80);
    let body = if version.is_empty() {
        "A new AeroForge build is ready to download.".to_string()
    } else {
        format!("{version} is ready to download.")
    };
    let script = build_windows_notification_script("AeroForge update available", &body);

    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not launch Windows notification helper: {error}"))
}

#[cfg(not(windows))]
fn show_desktop_update_notification(_version_label: &str) -> Result<(), String> {
    Err("Windows notifications are only available on Windows builds.".into())
}

#[cfg(windows)]
fn build_windows_notification_script(title: &str, body: &str) -> String {
    let title = powershell_single_quote(title);
    let body = powershell_single_quote(body);

    format!(
        r#"$ErrorActionPreference = 'Stop'
$title = {title}
$body = {body}
try {{
  [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
  [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > $null
  $template = [Windows.UI.Notifications.ToastTemplateType]::ToastText02
  $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent($template)
  $textNodes = $xml.GetElementsByTagName('text')
  [void]$textNodes.Item(0).AppendChild($xml.CreateTextNode($title))
  [void]$textNodes.Item(1).AppendChild($xml.CreateTextNode($body))
  $toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
  [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('AeroForge Control').Show($toast)
  exit 0
}} catch {{
  try {{
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $notify = New-Object System.Windows.Forms.NotifyIcon
    $notify.Icon = [System.Drawing.SystemIcons]::Information
    $notify.Visible = $true
    $notify.BalloonTipTitle = $title
    $notify.BalloonTipText = $body
    $notify.ShowBalloonTip(9000)
    Start-Sleep -Seconds 10
    $notify.Dispose()
    exit 0
  }} catch {{
    exit 1
  }}
}}"#
    )
}

fn normalize_notification_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(windows)]
fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[tauri::command]
pub fn apply_blue_light_filter(
    enabled: bool,
    state: State<'_, BackendState>,
) -> Result<BlueLightApplyResult, String> {
    let applied =
        blue_light::apply_blue_light_filter(enabled).map_err(|error| error.to_string())?;

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
    let applied = match service_pipe::apply_smart_charging(enabled) {
        Ok(applied) => smart_charge::SmartChargeApplyPayload {
            enabled: applied.enabled,
            battery_healthy: applied.battery_healthy,
            applied_at_unix: applied.applied_at_unix,
            detail: applied.detail,
        },
        Err(service_error) => smart_charge::apply_smart_charging(enabled)
            .await
            .map_err(|desktop_error| {
                format!(
                    "Service smart-charge apply failed: {service_error}. Desktop fallback also failed: {desktop_error}"
                )
            })?,
    };

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
pub fn apply_auto_refresh_rate(
    enabled: bool,
    on_battery: bool,
    state: State<'_, BackendState>,
) -> Result<super::models::DisplayRefreshApplyResult, String> {
    let mut controls = state.controls();
    let applied = display_refresh::sync_auto_refresh_rate(
        enabled,
        on_battery,
        controls.personal_settings.auto_refresh_rate_restore_hz,
    )
    .map_err(|error| error.to_string())?;

    controls
        .personal_settings
        .auto_refresh_rate_on_battery_enabled = applied.enabled;
    controls.personal_settings.auto_refresh_rate_restore_hz = applied.restore_hz;

    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    Ok(super::models::DisplayRefreshApplyResult {
        controls,
        applied_at_unix: applied.applied_at_unix,
        enabled: applied.enabled,
        on_battery: applied.on_battery,
        current_hz: applied.current_hz,
        applied_hz: applied.applied_hz,
        restore_hz: applied.restore_hz,
        detail: applied.detail,
    })
}

#[tauri::command]
pub fn set_nvidia_telemetry_enabled(
    enabled: bool,
    state: State<'_, BackendState>,
) -> Result<NvidiaTelemetryApplyResult, String> {
    let service_result = service_pipe::apply_telemetry_settings(enabled);

    let mut controls = state.controls();
    controls.personal_settings.nvidia_telemetry_enabled = enabled;
    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    let (enabled, detail) = match service_result {
        Ok(applied) => (applied.nvidia_telemetry_enabled, applied.detail),
        Err(error) => (
            enabled,
            format!(
                "Saved NVIDIA telemetry setting locally, but the service did not accept it yet: {error}"
            ),
        ),
    };

    Ok(NvidiaTelemetryApplyResult {
        controls,
        enabled,
        detail,
    })
}

fn merge_capabilities(
    desktop: CapabilitySnapshot,
    service: CapabilitySnapshot,
) -> CapabilitySnapshot {
    let mut notes = service.notes;
    for note in desktop.notes {
        if !notes.iter().any(|existing| existing == &note) {
            notes.push(note);
        }
    }

    CapabilitySnapshot {
        power_profiles: service.power_profiles,
        fan_profiles: service.fan_profiles,
        fan_curves: service.fan_curves,
        smart_charging: desktop.smart_charging,
        usb_power: desktop.usb_power,
        blue_light_filter: desktop.blue_light_filter,
        gpu_tuning: service.gpu_tuning,
        boot_logo: service.boot_logo,
        notes,
    }
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
pub async fn apply_power_profile(
    profile_id: PowerProfileId,
    processor_state: ProcessorStateSettings,
    custom_base_profile: Option<CustomPowerBaseId>,
    processor_state_control_enabled: bool,
    state: State<'_, BackendState>,
) -> Result<ControlSnapshot, String> {
    let custom_base_profile_for_apply = custom_base_profile.clone();
    let custom_base_profile_for_save = custom_base_profile.clone();
    let applied = tauri::async_runtime::spawn_blocking(move || {
        service_pipe::apply_power_profile(
            profile_id,
            processor_state,
            custom_base_profile_for_apply,
            processor_state_control_enabled,
        )
    })
    .await
    .map_err(|error| format!("Power profile apply worker failed: {error}"))?
    .map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.active_power_profile = applied.profile_id;
    controls.personal_settings.processor_state_control_enabled =
        applied.processor_state_control_enabled;
    if matches!(controls.active_power_profile, PowerProfileId::Custom) {
        controls.custom_processor_state = applied.processor_state;
        if let Some(custom_base_profile) = custom_base_profile_for_save {
            controls.custom_power_base = custom_base_profile;
        }
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
pub async fn apply_fan_profile(
    profile_id: FanProfileId,
    state: State<'_, BackendState>,
) -> Result<super::models::FanControlApplyResult, String> {
    let applied =
        tauri::async_runtime::spawn_blocking(move || service_pipe::apply_fan_profile(profile_id))
            .await
            .map_err(|error| format!("Fan profile apply worker failed: {error}"))?
            .map_err(|error| error.to_string())?;

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
pub async fn apply_custom_fan_curves(
    curves: FanCurveSet,
    state: State<'_, BackendState>,
) -> Result<super::models::FanControlApplyResult, String> {
    let curves_for_save = curves.clone();
    let applied = tauri::async_runtime::spawn_blocking(move || {
        service_pipe::apply_custom_fan_curves(curves.clone())
    })
    .await
    .map_err(|error| format!("Custom fan curve apply worker failed: {error}"))?
    .map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.active_fan_profile = FanProfileId::Custom;
    controls.fan_curves = applied.curves.unwrap_or(curves_for_save);

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
pub fn apply_boot_logo(
    file_name: String,
    image_base64: String,
    selected_boot_art: Option<String>,
    state: State<'_, BackendState>,
) -> Result<BootLogoApplyResult, String> {
    let image_path =
        boot_logo::save_uploaded_boot_logo(&state.config_root(), &file_name, &image_base64)
            .map_err(|error| error.to_string())?;
    let image_path_string = image_path.display().to_string();
    let applied = service_pipe::apply_boot_logo(image_path_string, Some(file_name.clone()))
        .map_err(|error| error.to_string())?;

    let mut controls = state.controls();
    controls.personal_settings.selected_boot_art = selected_boot_art
        .as_deref()
        .map(parse_boot_art_id)
        .unwrap_or(super::models::BootArtId::Custom);
    controls.personal_settings.custom_boot_filename = file_name;

    let controls = state
        .save_controls(controls)
        .map_err(|error| error.to_string())?;

    Ok(BootLogoApplyResult {
        controls,
        applied_at_unix: applied.applied_at_unix,
        detail: applied.detail,
    })
}

fn parse_boot_art_id(value: &str) -> super::models::BootArtId {
    match value {
        "ember" => super::models::BootArtId::Ember,
        "arc" => super::models::BootArtId::Arc,
        "slate" => super::models::BootArtId::Slate,
        _ => super::models::BootArtId::Custom,
    }
}

fn performance_log_path(state: &BackendState) -> io::Result<PathBuf> {
    let log_dir = state.config_root().join("logs");
    fs::create_dir_all(&log_dir)?;
    Ok(log_dir.join("performance.jsonl"))
}

fn rotate_performance_log_if_needed(path: &PathBuf) -> io::Result<()> {
    const MAX_PERFORMANCE_LOG_BYTES: u64 = 2 * 1024 * 1024;

    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };

    if metadata.len() < MAX_PERFORMANCE_LOG_BYTES {
        return Ok(());
    }

    let archived_path = path.with_extension("jsonl.old");
    let _ = fs::remove_file(&archived_path);
    fs::rename(path, archived_path)
}
