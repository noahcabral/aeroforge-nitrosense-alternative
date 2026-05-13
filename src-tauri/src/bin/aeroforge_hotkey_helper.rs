#![cfg(windows)]
#![windows_subsystem = "windows"]

use std::{
    collections::HashSet,
    env,
    fs::{self, OpenOptions},
    io::Write,
    mem::size_of,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    thread::{self, sleep},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use aeroforge_control_lib::backend::{
    commands::show_update_notification,
    models::UpdateChannelId,
    updater::{refresh_status, UpdaterStore},
};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_CLASS_ALREADY_EXISTS, HANDLE, HWND,
        INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        LibraryLoader::GetModuleHandleW,
        Threading::{
            AttachThreadInput, CreateMutexW, GetCurrentThreadId, OpenProcess,
            QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
    UI::{
        Input::{
            GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList,
            RegisterRawInputDevices, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER,
            RIDEV_INPUTSINK, RIDI_DEVICEINFO, RID_DEVICE_INFO, RID_INPUT, RIM_TYPEHID,
            RIM_TYPEKEYBOARD,
        },
        WindowsAndMessaging::{
            AllowSetForegroundWindow, BringWindowToTop, CreateWindowExW, DefWindowProcW,
            DispatchMessageW, EnumWindows, GetForegroundWindow, GetMessageW, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId, RegisterClassW, SetForegroundWindow,
            SetWindowPos, ShowWindow, ShowWindowAsync, SwitchToThisWindow, TranslateMessage,
            HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, MSG, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
            SW_RESTORE, WM_INPUT, WNDCLASSW, WS_OVERLAPPED,
        },
    },
};

const NITRO_KEY_VKEY: u16 = 0x00ff;
const NITRO_KEY_SCAN: u16 = 0x0075;
const RAW_KEY_BREAK: u16 = 0x0001;
const DEBOUNCE_MS: u64 = 750;
const ASFW_ANY: u32 = u32::MAX;
const BACKGROUND_UPDATE_INITIAL_DELAY: Duration = Duration::from_secs(30);
const BACKGROUND_UPDATE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

static LAST_NITRO_KEY_MS: AtomicU64 = AtomicU64::new(0);
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

    if let Err(error) = run_raw_input_loop(&log_path) {
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

fn run_background_update_check(
    log_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(config_root) = log_path.parent().map(Path::to_path_buf) else {
        return Err("could not resolve AeroForge config root".into());
    };
    fs::create_dir_all(&config_root)?;

    let settings = load_background_update_settings(&config_root);
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
    show_update_notification(version_label)
        .map_err(|error| format!("Windows update notification failed: {error}"))?;

    notify_state.last_notified_key = Some(notification_key.clone());
    notify_state.last_notified_at_unix = Some(unix_now());
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

fn load_background_update_settings(config_root: &Path) -> BackgroundUpdateSettings {
    let control_file = config_root.join("control-state.json");
    let Ok(raw) = fs::read_to_string(control_file) else {
        return BackgroundUpdateSettings::default();
    };
    let Ok(parsed) = serde_json::from_str::<BackgroundControlState>(&raw) else {
        return BackgroundUpdateSettings::default();
    };
    let Some(personal_settings) = parsed.personal_settings else {
        return BackgroundUpdateSettings::default();
    };

    BackgroundUpdateSettings {
        enabled: personal_settings.check_for_updates_on_launch,
        channel: personal_settings.update_channel,
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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone)]
struct BackgroundUpdateSettings {
    enabled: bool,
    channel: UpdateChannelId,
}

impl Default for BackgroundUpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            channel: UpdateChannelId::Stable,
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

fn run_raw_input_loop(log_path: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let class_name = to_wide("AeroForgeNitroKeyHelperRawInputWindow");
    let window_name = to_wide("AeroForge Nitro Key Helper");
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if instance.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }

    let window_class = WNDCLASSW {
        lpfnWndProc: Some(raw_input_window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { std::mem::zeroed() }
    };

    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_CLASS_ALREADY_EXISTS as i32) {
            return Err(error.into());
        }
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        )
    };
    if hwnd.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }

    let devices = build_raw_input_registrations(hwnd);
    if devices.is_empty() {
        return Err("no raw-input devices were available to register".into());
    }

    let registered = unsafe {
        RegisterRawInputDevices(
            devices.as_ptr(),
            devices.len() as u32,
            size_of::<RAWINPUTDEVICE>() as u32,
        )
    };
    if registered == 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    write_log(
        log_path,
        &format!(
            "raw-input helper armed with {} registration(s)",
            devices.len()
        ),
    );

    let mut message = unsafe { std::mem::zeroed::<MSG>() };
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    Ok(())
}

fn build_raw_input_registrations(hwnd: HWND) -> Vec<RAWINPUTDEVICE> {
    let mut devices = Vec::new();
    let mut seen = HashSet::<(u16, u16)>::new();

    add_registration(&mut devices, &mut seen, 0x01, 0x06, hwnd);
    add_registration(&mut devices, &mut seen, 0x0c, 0x01, hwnd);

    let mut count = 0u32;
    let list_item_size = size_of::<RAWINPUTDEVICELIST>() as u32;
    let first_result =
        unsafe { GetRawInputDeviceList(std::ptr::null_mut(), &mut count, list_item_size) };
    if first_result == u32::MAX || count == 0 {
        return devices;
    }

    let mut raw_devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let list_result =
        unsafe { GetRawInputDeviceList(raw_devices.as_mut_ptr(), &mut count, list_item_size) };
    if list_result == u32::MAX {
        return devices;
    }

    for raw_device in raw_devices.into_iter().take(list_result as usize) {
        if raw_device.dwType == RIM_TYPEKEYBOARD {
            add_registration(&mut devices, &mut seen, 0x01, 0x06, hwnd);
            continue;
        }

        if raw_device.dwType != RIM_TYPEHID {
            continue;
        }

        let Some(info) = read_raw_input_device_info(raw_device.hDevice) else {
            continue;
        };
        let hid = unsafe { info.Anonymous.hid };
        if hid.usUsagePage != 0 && hid.usUsage != 0 {
            add_registration(&mut devices, &mut seen, hid.usUsagePage, hid.usUsage, hwnd);
        }
    }

    devices
}

fn add_registration(
    devices: &mut Vec<RAWINPUTDEVICE>,
    seen: &mut HashSet<(u16, u16)>,
    usage_page: u16,
    usage: u16,
    hwnd: HWND,
) {
    if !seen.insert((usage_page, usage)) {
        return;
    }

    devices.push(RAWINPUTDEVICE {
        usUsagePage: usage_page,
        usUsage: usage,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    });
}

fn read_raw_input_device_info(handle: HANDLE) -> Option<RID_DEVICE_INFO> {
    let mut info = RID_DEVICE_INFO::default();
    info.cbSize = size_of::<RID_DEVICE_INFO>() as u32;
    let mut info_size = info.cbSize;
    let result = unsafe {
        GetRawInputDeviceInfoW(
            handle,
            RIDI_DEVICEINFO,
            (&mut info as *mut RID_DEVICE_INFO).cast(),
            &mut info_size,
        )
    };

    if result == u32::MAX {
        None
    } else {
        Some(info)
    }
}

unsafe extern "system" fn raw_input_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        handle_raw_input(lparam);
    }

    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn handle_raw_input(lparam: LPARAM) {
    let mut raw_size = 0u32;
    let header_size = size_of::<RAWINPUTHEADER>() as u32;
    let first_result = unsafe {
        GetRawInputData(
            lparam as _,
            RID_INPUT,
            std::ptr::null_mut(),
            &mut raw_size,
            header_size,
        )
    };
    if first_result == u32::MAX || raw_size == 0 {
        return;
    }

    let mut buffer = vec![0u8; raw_size as usize];
    let read_result = unsafe {
        GetRawInputData(
            lparam as _,
            RID_INPUT,
            buffer.as_mut_ptr().cast(),
            &mut raw_size,
            header_size,
        )
    };
    if read_result == u32::MAX {
        return;
    }

    let raw_input = unsafe { &*(buffer.as_ptr() as *const RAWINPUT) };
    if raw_input.header.dwType != RIM_TYPEKEYBOARD {
        return;
    }

    let keyboard = unsafe { raw_input.data.keyboard };
    let is_key_down = keyboard.Flags & RAW_KEY_BREAK == 0;
    if !is_key_down || keyboard.VKey != NITRO_KEY_VKEY || keyboard.MakeCode != NITRO_KEY_SCAN {
        return;
    }

    if !accept_debounced_press() {
        return;
    }

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

    if let Some(hwnd) = find_app_window(&app_exe) {
        let detail = restore_native_window_to_foreground(hwnd);
        write_log(log_path, &detail);
        return;
    }

    if is_app_process_running(&app_exe) {
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
        if let Some(hwnd) = find_app_window(app_exe) {
            return Some(hwnd);
        }
        sleep(Duration::from_millis(150));
    }
    None
}

fn is_app_process_running(app_exe: &Path) -> bool {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return false;
    }

    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) != 0 };
    while has_entry {
        if process_path_matches(entry.th32ProcessID, app_exe) {
            unsafe {
                CloseHandle(snapshot);
            }
            return true;
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) != 0 };
    }

    unsafe {
        CloseHandle(snapshot);
    }
    false
}

fn find_app_window(app_exe: &Path) -> Option<HWND> {
    let mut context = WindowSearchContext {
        app_exe: app_exe.to_path_buf(),
        hwnd: std::ptr::null_mut(),
    };

    unsafe {
        EnumWindows(
            Some(enum_windows_for_app),
            (&mut context as *mut WindowSearchContext) as LPARAM,
        );
    }

    if context.hwnd.is_null() {
        None
    } else {
        Some(context.hwnd)
    }
}

struct WindowSearchContext {
    app_exe: PathBuf,
    hwnd: HWND,
}

unsafe extern "system" fn enum_windows_for_app(hwnd: HWND, lparam: LPARAM) -> i32 {
    let context = unsafe { &mut *(lparam as *mut WindowSearchContext) };
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut process_id);
    }
    if process_id == 0 {
        return 1;
    }

    if process_path_matches(process_id, &context.app_exe)
        && window_title(hwnd).contains("AeroForge Control")
    {
        context.hwnd = hwnd;
        return 0;
    }

    1
}

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

fn process_path_matches(process_id: u32, target: &Path) -> bool {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return false;
    }

    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe {
        CloseHandle(process);
    }
    if ok == 0 {
        return false;
    }

    let process_path = String::from_utf16_lossy(&buffer[..length as usize]);
    paths_equal(Path::new(&process_path), target)
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left
        .canonicalize()
        .unwrap_or_else(|_| left.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase();
    let right = right
        .canonicalize()
        .unwrap_or_else(|_| right.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase();
    left == right
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

fn accept_debounced_press() -> bool {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let previous_ms = LAST_NITRO_KEY_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(previous_ms) < DEBOUNCE_MS {
        return false;
    }
    LAST_NITRO_KEY_MS.store(now_ms, Ordering::Relaxed);
    true
}

fn write_log(path: &Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {message}", timestamp_seconds());
    }
}

fn timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
