use std::{
    ffi::OsString,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStringExt,
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Devices::{
        DeviceAndDriverInstallation::{
            SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
            SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
            SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
        },
        HumanInterfaceDevice::{
            HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid, HidD_SetFeature, HIDD_ATTRIBUTES,
        },
    },
    Foundation::{
        CloseHandle, GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    },
};

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const ACER_VENDOR_ID: u16 = 0x1025;
const SYSTEM_USAGE_DEVICE_MARKER: &str = "hid#1025174b&col01#";
const REPORT_ID: u8 = 0xA0;
const REPORT_LEN: usize = 65;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemUsageMode {
    Turbo,
    Performance,
    Normal,
    Quiet,
}

impl SystemUsageMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Turbo => "turbo",
            Self::Performance => "performance",
            Self::Normal => "normal",
            Self::Quiet => "quiet",
        }
    }

    fn value(self) -> u8 {
        match self {
            Self::Turbo => 0,
            Self::Performance => 1,
            Self::Normal => 2,
            Self::Quiet => 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AcerHidWriteResult {
    pub action: &'static str,
    pub label: String,
    pub request_prefix: String,
    pub response_prefix: Option<String>,
}

pub fn apply_system_usage_mode(
    mode: SystemUsageMode,
) -> Result<AcerHidWriteResult, Box<dyn std::error::Error + Send + Sync>> {
    let request = build_system_usage_mode_request(mode);
    send_feature_report("system-usage-mode", mode.label(), &request, true)
}

pub fn apply_turbo_oc_profile_hint(
) -> Result<Vec<AcerHidWriteResult>, Box<dyn std::error::Error + Send + Sync>> {
    let app_status = build_app_status_request();
    let profile_select = build_oc_profile_select_request(0);
    Ok(vec![
        send_feature_report("app-status", "enable", &app_status, false)?,
        send_feature_report("oc-profile-select", "profile-0", &profile_select, false)?,
    ])
}

fn send_feature_report(
    action: &'static str,
    label: &str,
    request: &[u8; REPORT_LEN],
    read_response: bool,
) -> Result<AcerHidWriteResult, Box<dyn std::error::Error + Send + Sync>> {
    let device_path = find_system_usage_device_path()?;
    let handle = HidHandle(open_hid_handle(&device_path, GENERIC_READ | GENERIC_WRITE)?);

    let write_ok =
        unsafe { HidD_SetFeature(handle.0, request.as_ptr() as *const _, REPORT_LEN as u32) };
    if !write_ok {
        return Err(std::io::Error::last_os_error().into());
    }

    let response_prefix = if read_response {
        let mut response = [0u8; REPORT_LEN];
        response[0] = REPORT_ID;
        let read_ok = unsafe {
            HidD_GetFeature(handle.0, response.as_mut_ptr() as *mut _, REPORT_LEN as u32)
        };
        if read_ok {
            Some(format_hex_prefix(&response, 16))
        } else {
            None
        }
    } else {
        None
    };

    Ok(AcerHidWriteResult {
        action,
        label: label.into(),
        request_prefix: format_hex_prefix(request, 9),
        response_prefix,
    })
}

fn build_system_usage_mode_request(mode: SystemUsageMode) -> [u8; REPORT_LEN] {
    build_request([
        REPORT_ID,
        0x00,
        REPORT_ID,
        0x01,
        0x00,
        0x01,
        mode.value(),
        0x00,
        0x00,
    ])
}

fn build_app_status_request() -> [u8; REPORT_LEN] {
    build_request([
        REPORT_ID, 0x00, REPORT_ID, 0x03, 0x11, 0x01, 0x03, 0x01, 0x00,
    ])
}

fn build_oc_profile_select_request(profile: u8) -> [u8; REPORT_LEN] {
    build_request([
        REPORT_ID, 0x00, REPORT_ID, 0x02, 0x00, 0x01, profile, 0x00, 0x00,
    ])
}

fn build_request(prefix: [u8; 9]) -> [u8; REPORT_LEN] {
    let mut request = [0u8; REPORT_LEN];
    request[..prefix.len()].copy_from_slice(&prefix);
    request
}

fn find_system_usage_device_path() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut guid = unsafe { zeroed() };
    unsafe {
        HidD_GetHidGuid(&mut guid);
    }

    let info_set = unsafe {
        SetupDiGetClassDevsW(
            &guid,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };

    if info_set == INVALID_HANDLE_VALUE as isize {
        return Err(std::io::Error::last_os_error().into());
    }

    let info_set = DeviceInfoSet(info_set);
    let mut index = 0;

    loop {
        let mut interface_data = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..unsafe { zeroed() }
        };

        let ok = unsafe {
            SetupDiEnumDeviceInterfaces(info_set.0, null_mut(), &guid, index, &mut interface_data)
        };

        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_ITEMS {
                break;
            }
            return Err(std::io::Error::from_raw_os_error(error as i32).into());
        }

        if let Some(path) = try_system_usage_device_path(info_set.0, &interface_data)? {
            return Ok(path);
        }

        index += 1;
    }

    Err("Acer Nitro COL01 HID system-usage device was not found.".into())
}

fn try_system_usage_device_path(
    info_set: isize,
    interface_data: &SP_DEVICE_INTERFACE_DATA,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut required_size = 0u32;

    unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            info_set,
            interface_data,
            null_mut(),
            0,
            &mut required_size,
            null_mut(),
        );
    }

    let error = unsafe { GetLastError() };
    if required_size == 0 || error != ERROR_INSUFFICIENT_BUFFER {
        return Ok(None);
    }

    let mut detail_buffer = vec![0u8; required_size as usize];
    let detail_ptr = detail_buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
    unsafe {
        (*detail_ptr).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
    }

    let ok = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            info_set,
            interface_data,
            detail_ptr,
            required_size,
            &mut required_size,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let path = unsafe {
        read_null_terminated_wstr(std::ptr::addr_of!((*detail_ptr).DevicePath) as *const u16)
    };
    if !path
        .to_ascii_lowercase()
        .contains(SYSTEM_USAGE_DEVICE_MARKER)
    {
        return Ok(None);
    }

    let handle = open_hid_handle(&path, 0)?;
    let mut attributes = HIDD_ATTRIBUTES {
        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
        ..unsafe { zeroed() }
    };
    let attributes_ok = unsafe { HidD_GetAttributes(handle, &mut attributes) };
    unsafe {
        CloseHandle(handle);
    }

    if !attributes_ok || attributes.VendorID != ACER_VENDOR_ID {
        return Ok(None);
    }

    Ok(Some(path))
}

fn open_hid_handle(
    path: &str,
    desired_access: u32,
) -> Result<HANDLE, Box<dyn std::error::Error + Send + Sync>> {
    let mut wide: Vec<u16> = path.encode_utf16().collect();
    wide.push(0);

    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(handle)
    }
}

unsafe fn read_null_terminated_wstr(ptr: *const u16) -> String {
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    OsString::from_wide(std::slice::from_raw_parts(ptr, len))
        .to_string_lossy()
        .into_owned()
}

fn format_hex_prefix(bytes: &[u8], count: usize) -> String {
    bytes
        .iter()
        .take(count)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

struct DeviceInfoSet(isize);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

struct HidHandle(HANDLE);

impl Drop for HidHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_system_usage_mode_report_prefixes() {
        assert_eq!(
            &build_system_usage_mode_request(SystemUsageMode::Turbo)[..9],
            &[0xA0, 0x00, 0xA0, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            &build_system_usage_mode_request(SystemUsageMode::Quiet)[..9],
            &[0xA0, 0x00, 0xA0, 0x01, 0x00, 0x01, 0x03, 0x00, 0x00]
        );
    }

    #[test]
    fn builds_turbo_oc_hint_prefixes() {
        assert_eq!(
            &build_app_status_request()[..9],
            &[0xA0, 0x00, 0xA0, 0x03, 0x11, 0x01, 0x03, 0x01, 0x00]
        );
        assert_eq!(
            &build_oc_profile_select_request(0)[..9],
            &[0xA0, 0x00, 0xA0, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
    }
}
