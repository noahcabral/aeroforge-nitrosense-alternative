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

use super::models::AcerHidStatusSnapshot;

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const ACER_VENDOR_ID: u16 = 0x1025;
const STATUS_DEVICE_MARKER: &str = "hid#1025174b&col01#";
const STATUS_REPORT_LEN: usize = 65;
const STATUS_GROUP: u8 = 8;

pub fn read_status_snapshot() -> AcerHidStatusSnapshot {
    query_status_snapshot().unwrap_or_default()
}

fn query_status_snapshot() -> Result<AcerHidStatusSnapshot, Box<dyn std::error::Error + Send + Sync>>
{
    let device_path = find_status_device_path()?;
    let handle = open_hid_handle(&device_path, GENERIC_READ | GENERIC_WRITE)?;

    let snapshot = AcerHidStatusSnapshot {
        cpu_temp_c: query_selector(handle, 1).ok().and_then(to_u8),
        cpu_fan_rpm: query_selector(handle, 2).ok(),
        system_temp_c: query_selector(handle, 3).ok().and_then(to_u8),
        gpu_fan_rpm: query_selector(handle, 6).ok(),
    };

    unsafe {
        CloseHandle(handle);
    }

    Ok(snapshot)
}

fn to_u8(value: u16) -> Option<u8> {
    u8::try_from(value).ok()
}

fn find_status_device_path() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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

        if let Some(path) = try_status_device_path(info_set.0, &interface_data)? {
            return Ok(path);
        }

        index += 1;
    }

    Err("Acer Nitro HID status device was not found.".into())
}

fn try_status_device_path(
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
    if !path.to_ascii_lowercase().contains(STATUS_DEVICE_MARKER) {
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

fn query_selector(
    handle: HANDLE,
    selector: u8,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    let mut request = [0u8; STATUS_REPORT_LEN];
    request[0] = 0xA0;
    request[2] = 0xA0;
    request[3] = STATUS_GROUP;
    request[5] = 0x02;
    request[6] = selector;

    let write_ok = unsafe {
        HidD_SetFeature(
            handle,
            request.as_ptr() as *const _,
            STATUS_REPORT_LEN as u32,
        )
    };
    if !write_ok {
        return Err(std::io::Error::last_os_error().into());
    }

    let mut response = [0u8; STATUS_REPORT_LEN];
    response[0] = 0xA0;
    let read_ok = unsafe {
        HidD_GetFeature(
            handle,
            response.as_mut_ptr() as *mut _,
            STATUS_REPORT_LEN as u32,
        )
    };
    if !read_ok {
        return Err(std::io::Error::last_os_error().into());
    }

    Ok(u16::from_le_bytes([response[8], response[9]]))
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

struct DeviceInfoSet(isize);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
