use std::{
    env,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::mem::size_of;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
            QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
    UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_TOP,
        SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    },
};

const EXISTING_INSTANCE_WAIT: Duration = Duration::from_secs(5);

pub fn activate_existing_instance() -> bool {
    #[cfg(windows)]
    {
        activate_existing_instance_windows()
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn activate_existing_instance_windows() -> bool {
    let Ok(app_exe) = env::current_exe() else {
        return false;
    };
    let current_pid = unsafe { GetCurrentProcessId() };
    let deadline = Instant::now() + EXISTING_INSTANCE_WAIT;

    loop {
        if let Some(hwnd) = find_existing_main_window(&app_exe, current_pid) {
            restore_native_window_to_foreground(hwnd);
            return true;
        }

        if !is_app_process_running(&app_exe, current_pid) || Instant::now() >= deadline {
            return false;
        }

        sleep(Duration::from_millis(150));
    }
}

#[cfg(windows)]
fn find_existing_main_window(app_exe: &Path, current_pid: u32) -> Option<HWND> {
    let mut context = WindowSearchContext {
        app_exe: app_exe.to_path_buf(),
        current_pid,
        hwnd: std::ptr::null_mut(),
    };

    unsafe {
        EnumWindows(
            Some(enum_windows_for_existing_app),
            (&mut context as *mut WindowSearchContext) as LPARAM,
        );
    }

    if context.hwnd.is_null() {
        None
    } else {
        Some(context.hwnd)
    }
}

#[cfg(windows)]
struct WindowSearchContext {
    app_exe: PathBuf,
    current_pid: u32,
    hwnd: HWND,
}

#[cfg(windows)]
unsafe extern "system" fn enum_windows_for_existing_app(hwnd: HWND, lparam: LPARAM) -> i32 {
    let context = unsafe { &mut *(lparam as *mut WindowSearchContext) };
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut process_id);
    }

    if process_id == 0 || process_id == context.current_pid {
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

#[cfg(windows)]
fn is_app_process_running(app_exe: &Path, current_pid: u32) -> bool {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return false;
    }

    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) != 0 };
    while has_entry {
        if entry.th32ProcessID != current_pid && process_path_matches(entry.th32ProcessID, app_exe)
        {
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
fn restore_native_window_to_foreground(hwnd: HWND) {
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

    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
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
}
