use std::{
    env,
    mem::size_of,
    path::{Path, PathBuf},
    ptr::null_mut,
};

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
const WINRING_ENABLE_ENV: &str = "AEROFORGE_ENABLE_WINRING0";
const WINRING_DRIVER_PATH_ENV: &str = "AEROFORGE_WINRING0_DRIVER";
const IA32_THERM_STATUS: u32 = 0x19C;
const IA32_TEMPERATURE_TARGET: u32 = 0x1A2;
const IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;
pub(crate) const MSR_RAPL_POWER_UNIT: u32 = 0x606;
pub(crate) const MSR_PKG_POWER_LIMIT: u32 = 0x610;
pub(crate) const MSR_PKG_ENERGY_STATUS: u32 = 0x611;
const OLS_TYPE: u32 = 40000;
const METHOD_BUFFERED: u32 = 0;
const FILE_ANY_ACCESS: u32 = 0;
const IOCTL_OLS_READ_MSR: u32 = ctl_code(OLS_TYPE, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS);
const IOCTL_OLS_WRITE_MSR: u32 = ctl_code(OLS_TYPE, 0x822, METHOD_BUFFERED, FILE_ANY_ACCESS);
const POWER_LIMIT_RAW_MASK: u64 = 0x7fff;
const PL1_ENABLE_BIT: u8 = 15;
const PL2_ENABLE_BIT: u8 = 47;

const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

pub struct WinRingContext {
    pub driver_path: PathBuf,
    device: DeviceHandle,
}

#[derive(Clone, Copy, Debug)]
pub struct RaplReadback {
    pub package_energy_raw: u32,
    pub power_unit_w: f64,
    pub energy_unit_j: f64,
    pub package_power_limit: Option<PackagePowerLimitReadback>,
}

#[derive(Clone, Copy, Debug)]
pub struct PackagePowerLimitReadback {
    pub pl1_w: Option<f32>,
    pub pl1_enabled: bool,
    pub pl2_w: Option<f32>,
    pub pl2_enabled: bool,
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PackagePowerLimitWrite {
    pub pl1_w: Option<f32>,
    pub pl2_w: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct PackagePowerLimitApplyResult {
    pub before_raw: u64,
    pub target_raw: u64,
    pub after_raw: u64,
    pub power_unit_w: f64,
    pub before: PackagePowerLimitReadback,
    pub after: PackagePowerLimitReadback,
    pub changed: bool,
}

impl WinRingContext {
    pub fn load(paths: &ServicePaths) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if !winring_enabled() {
            cleanup_stale_staged_drivers(paths);
            return Err(
                "WinRing0 CPU MSR/RAPL path is disabled by default because Defender commonly flags the legacy driver. Set AEROFORGE_ENABLE_WINRING0=1 and provide AEROFORGE_WINRING0_DRIVER or a pre-staged ProgramData driver only for explicit diagnostics."
                    .into(),
            );
        }

        let driver_path = resolve_driver_binary(paths)?;
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

    pub fn read_rapl_readback(
        &self,
    ) -> Result<Option<RaplReadback>, Box<dyn std::error::Error + Send + Sync>> {
        let Some(unit_raw) = self.read_msr(MSR_RAPL_POWER_UNIT, 1)? else {
            return Ok(None);
        };
        let units = decode_rapl_units(unit_raw);

        let Some(package_energy_raw) = self.read_msr(MSR_PKG_ENERGY_STATUS, 1)? else {
            return Ok(None);
        };
        let package_power_limit = self
            .read_msr(MSR_PKG_POWER_LIMIT, 1)?
            .map(|raw| decode_package_power_limit(raw, units.power_unit_w));

        Ok(Some(RaplReadback {
            package_energy_raw: package_energy_raw as u32,
            power_unit_w: units.power_unit_w,
            energy_unit_j: units.energy_unit_j,
            package_power_limit,
        }))
    }

    pub fn apply_package_power_limit(
        &self,
        write: PackagePowerLimitWrite,
    ) -> Result<Option<PackagePowerLimitApplyResult>, Box<dyn std::error::Error + Send + Sync>>
    {
        let Some(unit_raw) = self.read_msr(MSR_RAPL_POWER_UNIT, 1)? else {
            return Ok(None);
        };
        let units = decode_rapl_units(unit_raw);
        let Some(before_raw) = self.read_msr(MSR_PKG_POWER_LIMIT, 1)? else {
            return Ok(None);
        };
        let before = decode_package_power_limit(before_raw, units.power_unit_w);
        if before.locked {
            return Err("CPU package power-limit MSR is locked by firmware.".into());
        }

        let target_raw = compose_package_power_limit_write(before_raw, write, units.power_unit_w)?;
        if target_raw != before_raw {
            self.write_msr(MSR_PKG_POWER_LIMIT, target_raw, 1)?;
        }

        let Some(after_raw) = self.read_msr(MSR_PKG_POWER_LIMIT, 1)? else {
            return Ok(None);
        };
        let after = decode_package_power_limit(after_raw, units.power_unit_w);
        verify_package_power_limit_write(target_raw, after_raw, write)?;

        Ok(Some(PackagePowerLimitApplyResult {
            before_raw,
            target_raw,
            after_raw,
            power_unit_w: units.power_unit_w,
            before,
            after,
            changed: target_raw != before_raw,
        }))
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

    fn write_msr(
        &self,
        register: u32,
        value: u64,
        affinity_mask: usize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current_thread = unsafe { GetCurrentThread() };
        let previous_mask = unsafe { SetThreadAffinityMask(current_thread, affinity_mask) };
        if previous_mask == 0 {
            return Err("Could not set thread affinity for MSR write.".into());
        }

        let result = self.device.write_msr(register, value);
        unsafe {
            let _ = SetThreadAffinityMask(current_thread, previous_mask);
        }

        result
    }
}

pub fn winring_enabled() -> bool {
    env::var(WINRING_ENABLE_ENV)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RaplUnits {
    pub power_unit_w: f64,
    pub energy_unit_j: f64,
}

pub(crate) fn decode_rapl_units(raw: u64) -> RaplUnits {
    let power_exponent = (raw & 0x0f) as i32;
    let energy_exponent = ((raw >> 8) & 0x1f) as i32;

    RaplUnits {
        power_unit_w: 1.0 / 2f64.powi(power_exponent),
        energy_unit_j: 1.0 / 2f64.powi(energy_exponent),
    }
}

pub(crate) fn decode_package_power_limit(raw: u64, power_unit_w: f64) -> PackagePowerLimitReadback {
    PackagePowerLimitReadback {
        pl1_w: decode_power_limit_w(raw, 0, power_unit_w),
        pl1_enabled: bit_set(raw, 15),
        pl2_w: decode_power_limit_w(raw, 32, power_unit_w),
        pl2_enabled: bit_set(raw, 47),
        locked: bit_set(raw, 63),
    }
}

fn decode_power_limit_w(raw: u64, shift: u8, power_unit_w: f64) -> Option<f32> {
    let encoded = ((raw >> shift) & 0x7fff) as u16;
    if encoded == 0 {
        return None;
    }

    let watts = encoded as f64 * power_unit_w;
    watts.is_finite().then_some(watts as f32)
}

pub(crate) fn compose_package_power_limit_write(
    current_raw: u64,
    write: PackagePowerLimitWrite,
    power_unit_w: f64,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let mut target = current_raw;

    if let Some(pl1_w) = write.pl1_w {
        let encoded = encode_power_limit_raw(pl1_w, power_unit_w)?;
        target &= !(POWER_LIMIT_RAW_MASK << 0);
        target |= encoded << 0;
        target = set_bit(target, PL1_ENABLE_BIT, true);
    }

    if let Some(pl2_w) = write.pl2_w {
        let encoded = encode_power_limit_raw(pl2_w, power_unit_w)?;
        target &= !(POWER_LIMIT_RAW_MASK << 32);
        target |= encoded << 32;
        target = set_bit(target, PL2_ENABLE_BIT, true);
    }

    let pl1_raw = (target >> 0) & POWER_LIMIT_RAW_MASK;
    let pl2_raw = (target >> 32) & POWER_LIMIT_RAW_MASK;
    if pl1_raw > 0 && pl2_raw > 0 && pl1_raw > pl2_raw {
        return Err(format!(
            "Refusing CPU power-limit write where PL1 {:.1}W exceeds PL2 {:.1}W.",
            pl1_raw as f64 * power_unit_w,
            pl2_raw as f64 * power_unit_w
        )
        .into());
    }

    Ok(target)
}

pub(crate) fn verify_package_power_limit_write(
    target_raw: u64,
    after_raw: u64,
    write: PackagePowerLimitWrite,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if write.pl1_w.is_some()
        && (after_raw & POWER_LIMIT_RAW_MASK) != (target_raw & POWER_LIMIT_RAW_MASK)
    {
        return Err(format!(
            "CPU PL1 write did not stick. Target raw 0x{:016X}, read back 0x{:016X}.",
            target_raw, after_raw
        )
        .into());
    }

    if write.pl2_w.is_some()
        && ((after_raw >> 32) & POWER_LIMIT_RAW_MASK) != ((target_raw >> 32) & POWER_LIMIT_RAW_MASK)
    {
        return Err(format!(
            "CPU PL2 write did not stick. Target raw 0x{:016X}, read back 0x{:016X}.",
            target_raw, after_raw
        )
        .into());
    }

    Ok(())
}

fn encode_power_limit_raw(
    watts: f32,
    power_unit_w: f64,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    if !watts.is_finite() || watts <= 0.0 {
        return Err(format!("Invalid CPU package power limit {watts}W.").into());
    }

    let encoded = (f64::from(watts) / power_unit_w).round();
    if encoded <= 0.0 || encoded > POWER_LIMIT_RAW_MASK as f64 {
        return Err(format!(
            "CPU package power limit {watts}W is outside the encodable RAPL range for unit {power_unit_w}W."
        )
        .into());
    }

    Ok(encoded as u64)
}

fn bit_set(raw: u64, bit: u8) -> bool {
    ((raw >> bit) & 0x1) == 1
}

fn set_bit(raw: u64, bit: u8, enabled: bool) -> u64 {
    if enabled {
        raw | (1u64 << bit)
    } else {
        raw & !(1u64 << bit)
    }
}

#[repr(C)]
struct MsrWriteInput {
    register: u32,
    eax: u32,
    edx: u32,
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

    fn write_msr(
        &self,
        register: u32,
        value: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut input = MsrWriteInput {
            register,
            eax: (value & 0xffff_ffff) as u32,
            edx: ((value >> 32) & 0xffff_ffff) as u32,
        };
        let mut bytes_returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_OLS_WRITE_MSR,
                (&mut input as *mut MsrWriteInput).cast(),
                size_of::<MsrWriteInput>() as u32,
                null_mut(),
                0,
                &mut bytes_returned,
                null_mut(),
            )
        };

        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(())
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

fn resolve_driver_binary(
    paths: &ServicePaths,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(driver_path) = env::var(WINRING_DRIVER_PATH_ENV) {
        let driver_path = PathBuf::from(driver_path.trim());
        if is_usable_driver_path(&driver_path) {
            return Ok(driver_path);
        }

        return Err(format!(
            "{WINRING_DRIVER_PATH_ENV} points to {}, but that driver file does not exist.",
            driver_path.display()
        )
        .into());
    }

    for staged_driver_path in staged_driver_candidates(paths) {
        if is_usable_driver_path(&staged_driver_path) {
            return Ok(staged_driver_path);
        }
    }

    Err(format!(
        "WinRing0 is enabled, but no external driver was provided. Set {WINRING_DRIVER_PATH_ENV} to a trusted WinRing0x64.sys path for diagnostics."
    )
    .into())
}

fn staged_driver_candidates(paths: &ServicePaths) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(root) = paths.state_dir.parent() {
        candidates.push(root.join("drivers").join("WinRing0x64.sys"));
    }
    candidates.push(paths.state_dir.join("drivers").join("WinRing0x64.sys"));
    candidates
}

fn cleanup_stale_staged_drivers(paths: &ServicePaths) {
    for path in staged_driver_candidates(paths) {
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn is_usable_driver_path(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.eq_ignore_ascii_case("WinRing0x64.sys"))
            .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_rapl_power_and_energy_units() {
        let units = decode_rapl_units(0x0e03);

        assert!((units.power_unit_w - 0.125).abs() < f64::EPSILON);
        assert!((units.energy_unit_j - (1.0 / 16384.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn decodes_package_power_limits() {
        let raw = 360u64 | (1u64 << 15) | (760u64 << 32) | (1u64 << 47) | (1u64 << 63);
        let limit = decode_package_power_limit(raw, 0.125);

        assert_eq!(limit.pl1_enabled, true);
        assert_eq!(limit.pl2_enabled, true);
        assert_eq!(limit.locked, true);
        assert!((limit.pl1_w.unwrap() - 45.0).abs() < f32::EPSILON);
        assert!((limit.pl2_w.unwrap() - 95.0).abs() < f32::EPSILON);
    }

    #[test]
    fn composes_package_limit_write_preserving_non_targets() {
        let raw = 440u64
            | (1u64 << PL1_ENABLE_BIT)
            | (0x4au64 << 17)
            | (920u64 << 32)
            | (1u64 << PL2_ENABLE_BIT)
            | (0x12u64 << 49);

        let target = compose_package_power_limit_write(
            raw,
            PackagePowerLimitWrite {
                pl1_w: Some(115.0),
                pl2_w: None,
            },
            0.125,
        )
        .unwrap();

        assert_eq!(target & POWER_LIMIT_RAW_MASK, 920);
        assert_eq!(
            target & !(POWER_LIMIT_RAW_MASK << 0),
            raw & !(POWER_LIMIT_RAW_MASK << 0)
        );
    }

    #[test]
    fn rejects_pl1_above_pl2() {
        let raw = 440u64 | (1u64 << PL1_ENABLE_BIT) | (920u64 << 32) | (1u64 << PL2_ENABLE_BIT);

        let error = compose_package_power_limit_write(
            raw,
            PackagePowerLimitWrite {
                pl1_w: Some(150.0),
                pl2_w: None,
            },
            0.125,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("PL1"));
        assert!(error.contains("exceeds PL2"));
    }
}
