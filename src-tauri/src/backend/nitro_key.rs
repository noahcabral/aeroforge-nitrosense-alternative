#[cfg(windows)]
use std::{
    collections::HashSet,
    env,
    fs::OpenOptions,
    io::Write,
    mem::size_of,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex, OnceLock,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use tauri::{AppHandle, Manager};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND as SysHwnd, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{
            RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
            REG_OPTION_NON_VOLATILE, REG_SZ,
        },
        Threading::{AttachThreadInput, GetCurrentThreadId, CREATE_NO_WINDOW},
    },
    UI::{
        Input::{
            GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList,
            RegisterRawInputDevices, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER,
            RIDEV_INPUTSINK, RIDI_DEVICEINFO, RID_DEVICE_INFO, RID_INPUT, RIM_TYPEHID,
            RIM_TYPEKEYBOARD,
        },
        WindowsAndMessaging::{
            BringWindowToTop, CreateWindowExW, DefWindowProcW, DispatchMessageW,
            GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, RegisterClassW,
            SetForegroundWindow, SetWindowPos, ShowWindow, TranslateMessage, HWND_TOP, MSG,
            SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, WM_INPUT, WNDCLASSW, WS_OVERLAPPED,
        },
    },
};

#[cfg(windows)]
const NITRO_KEY_VKEY: u16 = 0x00ff;
#[cfg(windows)]
const NITRO_KEY_SCAN: u16 = 0x0075;
#[cfg(windows)]
const RAW_KEY_BREAK: u16 = 0x0001;
#[cfg(windows)]
const DEBOUNCE_MS: u64 = 750;

#[cfg(windows)]
static NITRO_KEY_SENDER: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();
#[cfg(windows)]
static LAST_NITRO_KEY_MS: AtomicU64 = AtomicU64::new(0);

pub fn start(app_handle: tauri::AppHandle) {
    #[cfg(windows)]
    start_windows(app_handle);

    #[cfg(not(windows))]
    let _ = app_handle;
}

#[cfg(windows)]
fn start_windows(app_handle: AppHandle) {
    let (sender, receiver) = mpsc::channel::<()>();
    if NITRO_KEY_SENDER.set(Mutex::new(sender)).is_err() {
        return;
    }

    let log_path = app_handle
        .path()
        .app_config_dir()
        .ok()
        .map(|path| path.join("nitro-key-listener.log"));
    write_listener_log(&log_path, "starting Nitro key raw-input listener");
    start_hotkey_helper(&log_path);

    let event_log_path = log_path.clone();
    thread::spawn(move || {
        while receiver.recv().is_ok() {
            write_listener_log(&event_log_path, "captured Nitro key vk=0xff scan=0x75");
            bring_main_window_forward(&app_handle, &event_log_path);
        }
    });

    let listener_log_path = log_path;
    thread::spawn(move || {
        if let Err(error) = run_raw_input_loop(listener_log_path.clone()) {
            log::warn!("Nitro key raw-input listener stopped: {error}");
            write_listener_log(
                &listener_log_path,
                &format!("raw-input listener stopped: {error}"),
            );
        }
    });
}

#[cfg(windows)]
fn start_hotkey_helper(log_path: &Option<PathBuf>) {
    let helper_path = match env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("aeroforge-hotkey-helper.exe"))
    }) {
        Some(path) => path,
        None => {
            write_listener_log(log_path, "hotkey helper path could not be resolved");
            return;
        }
    };

    if !helper_path.exists() {
        write_listener_log(
            log_path,
            &format!("hotkey helper missing at {}", helper_path.display()),
        );
        return;
    }

    ensure_hotkey_helper_startup(&helper_path, log_path);

    match Command::new(&helper_path)
        .arg("--daemon")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        Ok(_) => write_listener_log(
            log_path,
            &format!("hotkey helper launch requested: {}", helper_path.display()),
        ),
        Err(error) => {
            write_listener_log(log_path, &format!("hotkey helper launch failed: {error}"))
        }
    }
}

#[cfg(windows)]
fn ensure_hotkey_helper_startup(helper_path: &PathBuf, log_path: &Option<PathBuf>) {
    let key_path = to_wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let mut run_key = std::ptr::null_mut();
    let create_result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut run_key,
            std::ptr::null_mut(),
        )
    };

    if create_result != 0 || run_key.is_null() {
        write_listener_log(
            log_path,
            &format!("hotkey helper startup registration failed: {create_result}"),
        );
        return;
    }

    let value_name = to_wide("AeroForgeHotkeyHelper");
    let command = format!("\"{}\" --daemon", helper_path.display());
    let command_wide = to_wide(&command);
    let data_bytes = unsafe {
        std::slice::from_raw_parts(
            command_wide.as_ptr().cast::<u8>(),
            command_wide.len() * std::mem::size_of::<u16>(),
        )
    };
    let set_result = unsafe {
        RegSetValueExW(
            run_key,
            value_name.as_ptr(),
            0,
            REG_SZ,
            data_bytes.as_ptr(),
            data_bytes.len() as u32,
        )
    };
    unsafe {
        RegCloseKey(run_key);
    }

    if set_result == 0 {
        write_listener_log(
            log_path,
            &format!("hotkey helper startup registered: {command}"),
        );
    } else {
        write_listener_log(
            log_path,
            &format!("hotkey helper startup value failed: {set_result}"),
        );
    }
}

#[cfg(windows)]
fn bring_main_window_forward(app_handle: &AppHandle, log_path: &Option<PathBuf>) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        match window.hwnd() {
            Ok(hwnd) => {
                let detail = restore_native_window_to_foreground(hwnd.0 as SysHwnd);
                write_listener_log(log_path, &detail);
            }
            Err(error) => {
                write_listener_log(log_path, &format!("failed to get AeroForge HWND: {error}"));
            }
        }
        log::info!("Nitro key pressed; AeroForge main window was focused.");
    } else {
        log::warn!("Nitro key pressed, but the AeroForge main window was not available.");
        write_listener_log(
            log_path,
            "main window unavailable during Nitro key focus request",
        );
    }
}

#[cfg(windows)]
fn run_raw_input_loop(
    log_path: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let class_name = to_wide("AeroForgeNitroKeyRawInputWindow");
    let window_name = to_wide("AeroForge Nitro Key Listener");
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

    let devices = build_raw_input_registrations(hwnd, &log_path);
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

    write_listener_log(
        &log_path,
        &format!(
            "raw-input listener armed with {} registration(s)",
            devices.len()
        ),
    );
    log::info!("Nitro key raw-input listener armed.");

    let mut message = unsafe { std::mem::zeroed::<MSG>() };
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    Ok(())
}

#[cfg(windows)]
fn build_raw_input_registrations(hwnd: SysHwnd, log_path: &Option<PathBuf>) -> Vec<RAWINPUTDEVICE> {
    let mut devices = Vec::new();
    let mut seen = HashSet::<(u16, u16)>::new();

    add_registration(&mut devices, &mut seen, 0x01, 0x06, hwnd);
    add_registration(&mut devices, &mut seen, 0x0c, 0x01, hwnd);

    let mut count = 0u32;
    let list_item_size = size_of::<RAWINPUTDEVICELIST>() as u32;
    let first_result =
        unsafe { GetRawInputDeviceList(std::ptr::null_mut(), &mut count, list_item_size) };
    if first_result == u32::MAX || count == 0 {
        write_listener_log(
            log_path,
            "raw-input device enumeration unavailable; using generic registrations",
        );
        return devices;
    }

    let mut raw_devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let list_result =
        unsafe { GetRawInputDeviceList(raw_devices.as_mut_ptr(), &mut count, list_item_size) };
    if list_result == u32::MAX {
        write_listener_log(
            log_path,
            "raw-input device list read failed; using generic registrations",
        );
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

#[cfg(windows)]
fn add_registration(
    devices: &mut Vec<RAWINPUTDEVICE>,
    seen: &mut HashSet<(u16, u16)>,
    usage_page: u16,
    usage: u16,
    hwnd: SysHwnd,
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

#[cfg(windows)]
fn read_raw_input_device_info(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> Option<RID_DEVICE_INFO> {
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

#[cfg(windows)]
unsafe extern "system" fn raw_input_window_proc(
    hwnd: SysHwnd,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        handle_raw_input(lparam);
    }

    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[cfg(windows)]
fn restore_native_window_to_foreground(hwnd: SysHwnd) -> String {
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

    let show_ok = unsafe { ShowWindow(hwnd, SW_RESTORE) != 0 };
    let pos_ok = unsafe {
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
    let top_ok = unsafe { BringWindowToTop(hwnd) != 0 };
    let foreground_ok = unsafe { SetForegroundWindow(hwnd) != 0 };

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

    format!(
        "foreground request show={show_ok} pos={pos_ok} top={top_ok} foreground={foreground_ok} current_thread={current_thread} target_thread={target_thread} foreground_thread={foreground_thread}"
    )
}

#[cfg(windows)]
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

    if let Some(sender) = NITRO_KEY_SENDER.get() {
        if let Ok(sender) = sender.lock() {
            let _ = sender.send(());
        }
    }
}

#[cfg(windows)]
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

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn write_listener_log(path: &Option<PathBuf>, message: &str) {
    let Some(path) = path else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {message}", timestamp_seconds());
    }
}

#[cfg(windows)]
fn timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
