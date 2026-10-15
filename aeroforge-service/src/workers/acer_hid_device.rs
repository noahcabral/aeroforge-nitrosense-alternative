//! The Acer Nitro "system usage" HID collection (VID 1025, COL01) is the one
//! device both the control writes and the status telemetry talk to, so the
//! discovery, handle and feature-report plumbing lives here once.

use std::{
    ffi::OsString,
    io,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStringExt,
    ptr::{null, null_mut},
    sync::{Mutex, MutexGuard},
};

use crate::DynError;

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

pub(super) const REPORT_ID: u8 = 0xA0;
pub(super) const REPORT_LEN: usize = 65;

type BoxError = DynError;

static CACHED_DEVICE_PATH: Mutex<Option<String>> = Mutex::new(None);
// A feature-report exchange is a set followed by a get on the same EC command
// slot, so a write from another thread in between would hand the reader the
// wrong response. One device, one exchange at a time.
static DEVICE_LOCK: Mutex<()> = Mutex::new(());

/// An open handle to the system-usage device; the exchange lock is held for its
/// lifetime and the handle closes on drop.
pub(super) struct SystemUsageDevice {
    handle: HANDLE,
    _exclusive: MutexGuard<'static, ()>,
}

impl SystemUsageDevice {
    pub(super) fn set_feature(&self, report: &[u8; REPORT_LEN]) -> io::Result<()> {
        let ok =
            unsafe { HidD_SetFeature(self.handle, report.as_ptr() as *const _, REPORT_LEN as u32) };
        if ok {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn get_feature(&self) -> io::Result<[u8; REPORT_LEN]> {
        let mut response = [0u8; REPORT_LEN];
        response[0] = REPORT_ID;
        let ok = unsafe {
            HidD_GetFeature(
                self.handle,
                response.as_mut_ptr() as *mut _,
                REPORT_LEN as u32,
            )
        };
        if ok {
            Ok(response)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

impl Drop for SystemUsageDevice {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// Opens the device for feature reports. The interface path is cached after the
/// first enumeration; a device that came back under a new path after a
/// reconnect fails the cached open and is re-enumerated once.
pub(super) fn open_system_usage_device() -> Result<SystemUsageDevice, BoxError> {
    let exclusive = DEVICE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cached = CACHED_DEVICE_PATH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let handle = match cached.map(|path| open_hid_handle(&path, GENERIC_READ | GENERIC_WRITE)) {
        Some(Ok(handle)) => handle,
        _ => {
            let path = find_system_usage_device_path()?;
            let handle = open_hid_handle(&path, GENERIC_READ | GENERIC_WRITE)?;
            *CACHED_DEVICE_PATH
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path);
            handle
        }
    };
    Ok(SystemUsageDevice {
        handle,
        _exclusive: exclusive,
    })
}

fn find_system_usage_device_path() -> Result<String, BoxError> {
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
        return Err(io::Error::last_os_error().into());
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
            return Err(io::Error::from_raw_os_error(error as i32).into());
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
) -> Result<Option<String>, BoxError> {
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
        return Err(io::Error::last_os_error().into());
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

    let handle = RawHandle(open_hid_handle(&path, 0)?);
    let mut attributes = HIDD_ATTRIBUTES {
        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
        ..unsafe { zeroed() }
    };
    let attributes_ok = unsafe { HidD_GetAttributes(handle.0, &mut attributes) };

    if !attributes_ok || attributes.VendorID != ACER_VENDOR_ID {
        return Ok(None);
    }

    Ok(Some(path))
}

fn open_hid_handle(path: &str, desired_access: u32) -> io::Result<HANDLE> {
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
        Err(io::Error::last_os_error())
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

struct RawHandle(HANDLE);

impl Drop for RawHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
