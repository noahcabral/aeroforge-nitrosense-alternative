#![cfg(windows)]
#![windows_subsystem = "windows"]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    thread::{self, sleep},
    time::{Duration, Instant},
};

use aeroforge_control_lib::backend::{
    commands::notify_update_available,
    log_file,
    models::UpdateChannelId,
    nitro_key, now_unix,
    single_instance::{find_existing_main_window, is_app_process_running},
    updater::{refresh_status, UpdaterStore},
};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND},
    System::Threading::{AttachThreadInput, CreateMutexW, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, SetForegroundWindow, SetWindowPos, ShowWindow,
        ShowWindowAsync, SwitchToThisWindow, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, SWP_NOMOVE,
        SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    },
};

const ASFW_ANY: u32 = u32::MAX;
const BACKGROUND_UPDATE_INITIAL_DELAY: Duration = Duration::from_secs(30);
const BACKGROUND_UPDATE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const UI_PREWARM_INITIAL_DELAY: Duration = Duration::from_secs(10);
const UI_PREWARM_INTERVAL: Duration = Duration::from_secs(30);
const UI_PREWARM_MAX_BACKOFF_EXPONENT: u32 = 5;

static APP_EXE: OnceLock<PathBuf> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn main() {
    let Some(log_path) = helper_log_path() else {
        return;
    };
    let _ = LOG_PATH.set(log_path.clone());

    let args: Vec<String> = env::args().skip(1).collect();
    let daemon_mode = args
        .iter()
        .any(|arg| arg == "--daemon" || arg == "--listen");
    let trigger_once = args.iter().any(|arg| arg == "--trigger");

    let Some(app_exe) = resolve_app_exe() else {
        write_log(
            &log_path,
            "unable to resolve aeroforge-control.exe beside helper",
        );
        return;
    };
    let _ = APP_EXE.set(app_exe.clone());

    if trigger_once || !daemon_mode {
        write_log(
            &log_path,
            if trigger_once {
                "manual trigger requested"
            } else {
                "one-shot activation requested"
            },
        );
        activate_or_launch(&log_path);
        return;
    }

    let Some(_mutex) = acquire_single_instance_mutex() else {
        write_log(&log_path, "another hotkey helper is already running");
        return;
    };

    write_log(
        &log_path,
        &format!("hotkey helper daemon started for {}", app_exe.display()),
    );
    spawn_background_update_worker(&log_path);
    spawn_ui_prewarm_worker(&log_path, &app_exe);

    if let Err(error) = nitro_key::run_raw_input_loop(on_nitro_key, Some(log_path.clone())) {
        write_log(&log_path, &format!("raw-input loop stopped: {error}"));
    }
}

fn acquire_single_instance_mutex() -> Option<HANDLE> {
    let name = to_wide(r"Local\AeroForgeNitroKeyHelper");
    let mutex = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
    if mutex.is_null() {
        return None;
    }

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(mutex);
        }
        return None;
    }

    Some(mutex)
}

fn resolve_app_exe() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|parent| parent.join("aeroforge-control.exe"))
        })
        .filter(|path| path.exists())
}

fn helper_log_path() -> Option<PathBuf> {
    env::var_os("APPDATA").map(PathBuf::from).map(|path| {
        path.join("com.noah.aeroforgecontrol")
            .join("nitro-key-helper.log")
    })
}

fn spawn_background_update_worker(log_path: &Path) {
    let log_path = log_path.to_path_buf();
    thread::spawn(move || {
        sleep(BACKGROUND_UPDATE_INITIAL_DELAY);
        loop {
            if let Err(error) = run_background_update_check(&log_path) {
                write_log(
                    &log_path,
                    &format!("background update check failed: {error}"),
                );
            }
            sleep(BACKGROUND_UPDATE_INTERVAL);
        }
    });
}

fn spawn_ui_prewarm_worker(log_path: &Path, app_exe: &Path) {
    let log_path = log_path.to_path_buf();
    let app_exe = app_exe.to_path_buf();
    thread::spawn(move || {
        sleep(UI_PREWARM_INITIAL_DELAY);
        let mut launched_last_tick = false;
        let mut failed_launches = 0u32;
        loop {
            let launched = match reconcile_ui_prewarm(&log_path, &app_exe) {
                Ok(launched) => launched,
                Err(error) => {
                    write_log(&log_path, &format!("UI prewarm check failed: {error}"));
                    true
                }
            };
            // A prewarm that is gone again by the next tick died at startup (missing WebView2,
            // crash): back off instead of relaunching a broken install every 30 seconds forever.
            failed_launches = if launched && launched_last_tick {
                failed_launches + 1
            } else if launched {
                failed_launches
            } else {
                0
            };
            launched_last_tick = launched;
            let backoff = UI_PREWARM_INTERVAL
                * 2u32.pow(failed_launches.min(UI_PREWARM_MAX_BACKOFF_EXPONENT));
            if failed_launches > 0 {
                write_log(
                    &log_path,
                    &format!(
                        "UI prewarm exited before the next check ({failed_launches} in a row); next attempt in {}s",
                        backoff.as_secs()
                    ),
                );
            }
            sleep(backoff);
        }
    });
}

#[cfg(test)]
mod prewarm_backoff_tests {
    use super::{UI_PREWARM_INTERVAL, UI_PREWARM_MAX_BACKOFF_EXPONENT};

    #[test]
    fn backoff_caps_at_max_exponent() {
        let backoff = |failed: u32| {
            UI_PREWARM_INTERVAL * 2u32.pow(failed.min(UI_PREWARM_MAX_BACKOFF_EXPONENT))
        };
        assert_eq!(backoff(0), UI_PREWARM_INTERVAL);
        assert_eq!(backoff(1), UI_PREWARM_INTERVAL * 2);
        assert_eq!(backoff(5), UI_PREWARM_INTERVAL * 32);
        assert_eq!(backoff(40), UI_PREWARM_INTERVAL * 32);
    }
}

/// Returns whether a hidden UI instance was launched on this tick.
fn reconcile_ui_prewarm(
    log_path: &Path,
    app_exe: &Path,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let Some(config_root) = log_path.parent().map(Path::to_path_buf) else {
        return Err("could not resolve AeroForge config root".into());
    };

    let settings = load_helper_settings(&config_root);
    if !settings.keep_ui_prewarmed {
        return Ok(false);
    }

    if is_app_process_running(app_exe, 0) {
        return Ok(false);
    }

    match Command::new(app_exe).arg("--prewarm").spawn() {
        Ok(child) => {
            write_log(
                log_path,
                &format!(
                    "launched hidden UI prewarm {} pid={}",
                    app_exe.display(),
                    child.id()
                ),
            );
            Ok(true)
        }
        Err(error) => Err(format!("failed to launch hidden UI prewarm: {error}").into()),
    }
}

fn run_background_update_check(
    log_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(config_root) = log_path.parent().map(Path::to_path_buf) else {
        return Err("could not resolve AeroForge config root".into());
    };
    fs::create_dir_all(&config_root)?;

    let settings = load_helper_settings(&config_root);
    if !settings.enabled {
        write_log(
            log_path,
            "background update check skipped because update checks are disabled",
        );
        return Ok(());
    }

    let updater = UpdaterStore::load(&config_root)?;
    let status = refresh_status(&updater, settings.channel.clone())?;
    if !status.update_available {
        write_log(
            log_path,
            &format!(
                "background update check found no {} update",
                settings.channel.as_str()
            ),
        );
        return Ok(());
    }

    let notification_key = background_update_notification_key(&status);
    let mut notify_state = load_background_update_state(&config_root);
    if notify_state.last_notified_key.as_deref() == Some(notification_key.as_str()) {
        write_log(
            log_path,
            &format!("background update notification already shown for {notification_key}"),
        );
        return Ok(());
    }

    let version_label = status
        .latest_version
        .clone()
        .or_else(|| status.latest_title.clone())
        .unwrap_or_else(|| "A new AeroForge update".into());
    notify_update_available(&version_label)
        .map_err(|error| format!("Windows update notification failed: {error}"))?;

    notify_state.last_notified_key = Some(notification_key.clone());
    notify_state.last_notified_at_unix = Some(now_unix());
    save_background_update_state(&config_root, &notify_state)?;
    write_log(
        log_path,
        &format!("background update notification shown for {notification_key}"),
    );

    Ok(())
}

fn background_update_notification_key(
    status: &aeroforge_control_lib::backend::models::UpdateStatus,
) -> String {
    let version = status
        .latest_version
        .as_deref()
        .or(status.latest_title.as_deref())
        .unwrap_or("unknown-version");
    let asset = status.latest_asset_name.as_deref().unwrap_or("no-asset");
    format!("{version}|{asset}")
}

fn load_helper_settings(config_root: &Path) -> HelperSettings {
    let control_file = config_root.join("control-state.json");
    let Ok(raw) = fs::read_to_string(control_file) else {
        return HelperSettings::default();
    };
    let Ok(parsed) = serde_json::from_str::<BackgroundControlState>(&raw) else {
        return HelperSettings::default();
    };
    let Some(personal_settings) = parsed.personal_settings else {
        return HelperSettings::default();
    };

    HelperSettings {
        enabled: personal_settings.check_for_updates_on_launch,
        channel: personal_settings.update_channel,
        keep_ui_prewarmed: personal_settings.keep_ui_prewarmed,
    }
}

fn load_background_update_state(config_root: &Path) -> BackgroundUpdateNotificationState {
    let state_file = config_root.join("background-update-notification.json");
    fs::read_to_string(state_file)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_background_update_state(
    config_root: &Path,
    state: &BackgroundUpdateNotificationState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state_file = config_root.join("background-update-notification.json");
    fs::write(state_file, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

#[derive(Debug, Clone)]
struct HelperSettings {
    enabled: bool,
    channel: UpdateChannelId,
    keep_ui_prewarmed: bool,
}

impl Default for HelperSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            channel: UpdateChannelId::Stable,
            keep_ui_prewarmed: false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundControlState {
    personal_settings: Option<BackgroundPersonalSettings>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundPersonalSettings {
    #[serde(default = "default_true")]
    check_for_updates_on_launch: bool,
    #[serde(default = "default_stable_channel")]
    update_channel: UpdateChannelId,
    #[serde(default)]
    keep_ui_prewarmed: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundUpdateNotificationState {
    last_notified_key: Option<String>,
    last_notified_at_unix: Option<u64>,
}

fn default_true() -> bool {
    true
}

fn default_stable_channel() -> UpdateChannelId {
    UpdateChannelId::Stable
}

fn on_nitro_key() {
    let Some(log_path) = LOG_PATH.get().cloned() else {
        return;
    };
    write_log(&log_path, "captured Nitro key vk=0xff scan=0x75");
    activate_or_launch(&log_path);
}

fn activate_or_launch(log_path: &Path) {
    let Some(app_exe) = APP_EXE.get().cloned() else {
        write_log(log_path, "app exe missing from helper state");
        return;
    };

    if let Some(hwnd) = find_existing_main_window(&app_exe, 0) {
        let detail = restore_native_window_to_foreground(hwnd);
        write_log(log_path, &detail);
        return;
    }

    if is_app_process_running(&app_exe, 0) {
        write_log(
            log_path,
            "app process is already running; waiting for its window instead of launching another copy",
        );
        if let Some(hwnd) = wait_for_app_window(&app_exe, Duration::from_secs(8)) {
            let detail = restore_native_window_to_foreground(hwnd);
            write_log(log_path, &format!("post-wait {detail}"));
        } else {
            write_log(
                log_path,
                "app window did not appear while process was already running; launching a fresh activation attempt",
            );
            match Command::new(&app_exe).spawn() {
                Ok(child) => {
                    write_log(
                        log_path,
                        &format!("fallback launched {} pid={}", app_exe.display(), child.id()),
                    );
                    let allow_foreground = unsafe { AllowSetForegroundWindow(child.id()) != 0 };
                    write_log(
                        log_path,
                        &format!(
                            "fallback allow foreground for launched process={allow_foreground}"
                        ),
                    );
                    if let Some(hwnd) = wait_for_app_window(&app_exe, Duration::from_secs(10)) {
                        let detail = restore_native_window_to_foreground(hwnd);
                        write_log(log_path, &format!("fallback post-launch {detail}"));
                    } else {
                        write_log(
                            log_path,
                            "fallback launched app but no visible window appeared",
                        );
                    }
                }
                Err(error) => write_log(
                    log_path,
                    &format!("fallback failed to launch {}: {error}", app_exe.display()),
                ),
            }
        }
        return;
    }

    match Command::new(&app_exe).spawn() {
        Ok(child) => {
            write_log(
                log_path,
                &format!("launched {} pid={}", app_exe.display(), child.id()),
            );
            let allow_foreground = unsafe { AllowSetForegroundWindow(child.id()) != 0 };
            write_log(
                log_path,
                &format!("allow foreground for launched process={allow_foreground}"),
            );
            if let Some(hwnd) = wait_for_app_window(&app_exe, Duration::from_secs(10)) {
                let detail = restore_native_window_to_foreground(hwnd);
                write_log(log_path, &format!("post-launch {detail}"));
            } else {
                write_log(log_path, "launched app but no visible window appeared");
            }
        }
        Err(error) => write_log(
            log_path,
            &format!("failed to launch {}: {error}", app_exe.display()),
        ),
    }
}

fn wait_for_app_window(app_exe: &Path, timeout: Duration) -> Option<HWND> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(hwnd) = find_existing_main_window(app_exe, 0) {
            return Some(hwnd);
        }
        sleep(Duration::from_millis(150));
    }
    None
}

// HWND is a raw pointer, so this stays local rather than becoming a pub lib fn.
fn window_title(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if copied <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buffer[..copied as usize])
    }
}

fn window_process_id(hwnd: HWND) -> u32 {
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut process_id);
    }
    process_id
}

fn restore_native_window_to_foreground(hwnd: HWND) -> String {
    let foreground_hwnd = unsafe { GetForegroundWindow() };
    let current_thread = unsafe { GetCurrentThreadId() };
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    let foreground_thread = if foreground_hwnd.is_null() {
        0
    } else {
        unsafe { GetWindowThreadProcessId(foreground_hwnd, std::ptr::null_mut()) }
    };

    let attach_target = target_thread != 0 && target_thread != current_thread;
    let attach_foreground = foreground_thread != 0
        && foreground_thread != current_thread
        && foreground_thread != target_thread;

    if attach_target {
        unsafe {
            AttachThreadInput(current_thread, target_thread, 1);
        }
    }
    if attach_foreground {
        unsafe {
            AttachThreadInput(current_thread, foreground_thread, 1);
        }
    }

    let foreground_unlock_ok = unsafe { AllowSetForegroundWindow(ASFW_ANY) != 0 };
    let mut show_ok = false;
    let mut async_show_ok = false;
    let mut topmost_ok = false;
    let mut notopmost_ok = false;
    let mut pos_ok = false;
    let mut top_ok = false;
    let mut foreground_ok = false;
    let mut foreground_matched = false;
    for _ in 0..12 {
        async_show_ok = unsafe { ShowWindowAsync(hwnd, SW_RESTORE) != 0 };
        show_ok = unsafe { ShowWindow(hwnd, SW_RESTORE) != 0 };
        topmost_ok = unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            ) != 0
        };
        notopmost_ok = unsafe {
            SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            ) != 0
        };
        pos_ok = unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            ) != 0
        };
        top_ok = unsafe { BringWindowToTop(hwnd) != 0 };
        foreground_ok = unsafe { SetForegroundWindow(hwnd) != 0 };
        unsafe {
            SwitchToThisWindow(hwnd, 1);
        }
        if unsafe { GetForegroundWindow() } == hwnd {
            foreground_matched = true;
            break;
        }
        sleep(Duration::from_millis(80));
    }

    if attach_foreground {
        unsafe {
            AttachThreadInput(current_thread, foreground_thread, 0);
        }
    }
    if attach_target {
        unsafe {
            AttachThreadInput(current_thread, target_thread, 0);
        }
    }

    let after_foreground_hwnd = unsafe { GetForegroundWindow() };
    let after_process_id = window_process_id(after_foreground_hwnd);
    let after_title = window_title(after_foreground_hwnd);

    format!(
        "helper foreground request unlock={foreground_unlock_ok} show={show_ok} async_show={async_show_ok} topmost={topmost_ok} notopmost={notopmost_ok} pos={pos_ok} top={top_ok} foreground={foreground_ok} matched={foreground_matched} title=\"{}\" after_foreground_pid={after_process_id} after_foreground_title=\"{after_title}\" current_thread={current_thread} target_thread={target_thread} foreground_thread={foreground_thread}",
        window_title(hwnd)
    )
}

fn write_log(path: &Path, message: &str) {
    log_file::append_line(path, message);
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
