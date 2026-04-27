use std::{thread, time::Duration};

const SWEEP_INTERVAL: Duration = Duration::from_secs(30);
#[cfg(windows)]
const NITRO_ROOT: &str = r"c:\program files\nitrosense\";
#[cfg(windows)]
const NITRO_EXE: &str = "nitrosense.exe";
#[cfg(windows)]
const NITRO_LAUNCHER_EXE: &str = "nitrosenselauncher.exe";

pub fn start() {
    #[cfg(windows)]
    {
        sweep_once();
        thread::spawn(|| loop {
            thread::sleep(SWEEP_INTERVAL);
            sweep_once();
        });
    }
}

#[cfg(windows)]
fn sweep_once() {
    match terminate_nitro_processes() {
        Ok(killed) if killed > 0 => {
            log::info!("Nitro guard terminated {killed} NitroSense-related process(es).");
        }
        Ok(_) => {}
        Err(error) => {
            log::warn!("Nitro guard sweep failed: {error}");
        }
    }
}

#[cfg(windows)]
fn terminate_nitro_processes() -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            },
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().into());
    }

    let mut killed = 0usize;
    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) != 0 };
    while has_entry {
        let exe_name = wide_to_string(&entry.szExeFile);
        let exe_name_lower = exe_name.to_ascii_lowercase();
        let pid = entry.th32ProcessID;

        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                0,
                pid,
            )
        };

        if !process.is_null() {
            let path = query_process_image_path(process);
            let should_kill = exe_name_lower == NITRO_EXE
                || exe_name_lower == NITRO_LAUNCHER_EXE
                || path
                    .as_deref()
                    .map(|value| value.to_ascii_lowercase().starts_with(NITRO_ROOT))
                    .unwrap_or(false);

            if should_kill {
                let terminated = unsafe { TerminateProcess(process, 1) != 0 };
                if terminated {
                    killed += 1;
                }
            }

            unsafe {
                CloseHandle(process);
            }
        }

        has_entry = unsafe { Process32NextW(snapshot, &mut entry) != 0 };
    }

    unsafe {
        CloseHandle(snapshot);
    }

    Ok(killed)
}

#[cfg(windows)]
fn wide_to_string(buffer: &[u16]) -> String {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};

    let length = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..length])
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
fn query_process_image_path(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;

    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    if ok == 0 {
        return None;
    }

    Some(
        OsString::from_wide(&buffer[..length as usize])
            .to_string_lossy()
            .into_owned(),
    )
}
