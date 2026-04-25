use std::{mem::size_of, path::PathBuf, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_EXISTS, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE, OPEN_EXISTING,
    },
    System::{
        Services::{
            CloseServiceHandle, CreateServiceW, OpenSCManagerW, OpenServiceW, StartServiceW,
            SC_MANAGER_ALL_ACCESS, SERVICE_ALL_ACCESS, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL,
            SERVICE_KERNEL_DRIVER,
        },
        Threading::{GetCurrentThread, SetThreadAffinityMask},
        IO::DeviceIoControl,
    },
};

use crate::paths::ServicePaths;

pub const RELATION_PROCESSOR_CORE: i32 = 0;

const WINRING_SERVICE_NAME: &str = "WinRing0_1_2_0";
const WINRING_DEVICE_PATH: &str = r"\\.\WinRing0_1_2_0";
const IA32_THERM_STATUS: u32 = 0x19C;
const IA32_TEMPERATURE_TARGET: u32 = 0x1A2;
const IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const OLS_TYPE: u32 = 40000;
const METHOD_BUFFERED: u32 = 0;
const FILE_ANY_ACCESS: u32 = 0;
const IOCTL_OLS_READ_MSR: u32 = ctl_code(OLS_TYPE, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS);
const WINRING_X64_SYS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/winring0/WinRing0x64.sys"
));

const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

pub struct WinRingContext {
    pub driver_path: PathBuf,
    device: DeviceHandle,
}

impl WinRingContext {
    pub fn load(paths: &ServicePaths) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let driver_path = stage_driver_binary(paths)?;
        ensure_driver_service(&driver_path)?;
        let device = DeviceHandle::open(WINRING_DEVICE_PATH)?;

        Ok(Self {
            driver_path,
            device,
        })
    }

    pub fn read_tj_max(&self) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let Some(value) = self.read_msr(IA32_TEMPERATURE_TARGET, 1)? else {
            return Ok(None);
        };

        let tj_max = ((value as u32 >> 16) & 0xff) as u8;
        Ok((tj_max > 0).then_some(tj_max))
    }

    pub fn read_package_temp(
        &self,
        tj_max_c: Option<u8>,
    ) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
        self.read_digital_thermal_temp(IA32_PACKAGE_THERM_STATUS, 1, tj_max_c)
    }

    pub fn read_core_temp(
        &self,
        affinity_mask: usize,
        tj_max_c: Option<u8>,
    ) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
        self.read_digital_thermal_temp(IA32_THERM_STATUS, affinity_mask, tj_max_c)
    }

    fn read_digital_thermal_temp(
        &self,
        register: u32,
        affinity_mask: usize,
        tj_max_c: Option<u8>,
    ) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let Some(tj_max_c) = tj_max_c else {
            return Ok(None);
        };

        let Some(value) = self.read_msr(register, affinity_mask)? else {
            return Ok(None);
        };

        let valid = ((value >> 31) & 0x1) == 0x1;
        if !valid {
            return Ok(None);
        }

        let delta_to_tj_max = ((value >> 16) & 0x7f) as u8;
        Ok(Some(tj_max_c.saturating_sub(delta_to_tj_max)))
    }

    fn read_msr(
        &self,
        register: u32,
        affinity_mask: usize,
    ) -> Result<Option<u64>, Box<dyn std::error::Error + Send + Sync>> {
        let current_thread = unsafe { GetCurrentThread() };
        let previous_mask = unsafe { SetThreadAffinityMask(current_thread, affinity_mask) };
        if previous_mask == 0 {
            return Ok(None);
        }

        let result = self.device.read_msr(register);
        unsafe {
            let _ = SetThreadAffinityMask(current_thread, previous_mask);
        }

        result
    }
}

struct DeviceHandle {
    handle: HANDLE,
}

impl DeviceHandle {
    fn open(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path_wide = to_wide_string(path);
        let handle = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                0,
                null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(Self { handle })
    }

    fn read_msr(
        &self,
        register: u32,
    ) -> Result<Option<u64>, Box<dyn std::error::Error + Send + Sync>> {
        let mut output = 0u64;
        let mut bytes_returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_OLS_READ_MSR,
                (&register as *const u32).cast_mut().cast(),
                size_of::<u32>() as u32,
                (&mut output as *mut u64).cast(),
                size_of::<u64>() as u32,
                &mut bytes_returned,
                null_mut(),
            )
        };

        if ok == 0 {
            return Ok(None);
        }

        Ok(Some(output))
    }
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

fn stage_driver_binary(
    paths: &ServicePaths,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let driver_dir = paths.state_dir.join("drivers");
    std::fs::create_dir_all(&driver_dir)?;
    let driver_path = driver_dir.join("WinRing0x64.sys");

    let should_write = match std::fs::metadata(&driver_path) {
        Ok(metadata) => metadata.len() != WINRING_X64_SYS.len() as u64,
        Err(_) => true,
    };

    if should_write {
        std::fs::write(&driver_path, WINRING_X64_SYS)?;
    }

    Ok(driver_path)
}

fn ensure_driver_service(
    driver_path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let manager = ServiceHandle::open_manager()?;
    let service =
        match ServiceHandle::create_kernel_driver(&manager, WINRING_SERVICE_NAME, driver_path) {
            Ok(service) => service,
            Err(error) if error.raw_os_error() == Some(ERROR_SERVICE_EXISTS as i32) => {
                ServiceHandle::open_service(&manager, WINRING_SERVICE_NAME)?
            }
            Err(error) => return Err(error.into()),
        };

    if let Err(error) = service.start() {
        if error.raw_os_error() != Some(ERROR_SERVICE_ALREADY_RUNNING as i32) {
            return Err(error.into());
        }
    }

    Ok(())
}

struct ServiceHandle {
    handle: HANDLE,
}

impl ServiceHandle {
    fn open_manager() -> Result<Self, std::io::Error> {
        let handle = unsafe { OpenSCManagerW(null_mut(), null_mut(), SC_MANAGER_ALL_ACCESS) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    fn create_kernel_driver(
        manager: &ServiceHandle,
        name: &str,
        driver_path: &PathBuf,
    ) -> Result<Self, std::io::Error> {
        let name_wide = to_wide_string(name);
        let path_wide = to_wide_string(&driver_path.display().to_string());
        let handle = unsafe {
            CreateServiceW(
                manager.handle,
                name_wide.as_ptr(),
                name_wide.as_ptr(),
                SERVICE_ALL_ACCESS,
                SERVICE_KERNEL_DRIVER,
                SERVICE_DEMAND_START,
                SERVICE_ERROR_NORMAL,
                path_wide.as_ptr(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };

        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        Ok(Self { handle })
    }

    fn open_service(manager: &ServiceHandle, name: &str) -> Result<Self, std::io::Error> {
        let name_wide = to_wide_string(name);
        let handle =
            unsafe { OpenServiceW(manager.handle, name_wide.as_ptr(), SERVICE_ALL_ACCESS) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    fn start(&self) -> Result<(), std::io::Error> {
        let ok = unsafe { StartServiceW(self.handle, 0, null_mut()) };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                let _ = CloseServiceHandle(self.handle);
            }
        }
    }
}

fn to_wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
