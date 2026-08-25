use std::{
    env,
    ffi::CString,
    path::{Path, PathBuf},
    ptr::null_mut,
};

use libloading::Library;
use windows_sys::Win32::Foundation::HANDLE;

use crate::paths::ServicePaths;

use super::winring::{
    self, PackagePowerLimitApplyResult, PackagePowerLimitWrite, RaplReadback,
    MSR_PKG_ENERGY_STATUS, MSR_PKG_POWER_LIMIT, MSR_RAPL_POWER_UNIT,
};

const PAWNIO_ENABLE_ENV: &str = "AEROFORGE_ENABLE_PAWNIO";
const PAWNIO_DLL_ENV: &str = "AEROFORGE_PAWNIO_DLL";
const PAWNIO_MODULE_ENV: &str = "AEROFORGE_PAWNIO_MODULE";
const PAWNIO_DEFAULT_MODULE_NAME: &str = "IntelMSR.bin";
const IOCTL_READ_MSR: &str = "ioctl_read_msr";
const IOCTL_WRITE_MSR: &str = "ioctl_write_msr";

type PawnIoOpen = unsafe extern "system" fn(*mut HANDLE) -> i32;
type PawnIoLoad = unsafe extern "system" fn(HANDLE, *const u8, usize) -> i32;
type PawnIoExecute = unsafe extern "system" fn(
    HANDLE,
    *const i8,
    *const u64,
    usize,
    *mut u64,
    usize,
    *mut usize,
) -> i32;
type PawnIoClose = unsafe extern "system" fn(HANDLE) -> i32;

pub struct PawnIoContext {
    pub module_path: PathBuf,
    _dll_path: PathBuf,
    _library: Library,
    api: PawnIoApi,
    handle: HANDLE,
}

struct PawnIoApi {
    execute: PawnIoExecute,
    close: PawnIoClose,
}

impl PawnIoContext {
    pub fn load(paths: &ServicePaths) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let explicit_enabled = pawnio_env_flag().unwrap_or(false);
        if pawnio_env_disabled() {
            return Err("PawnIO MSR/RAPL path is disabled by AEROFORGE_ENABLE_PAWNIO=0.".into());
        }

        let dll_path = match resolve_dll_path(paths) {
            Ok(path) => path,
            Err(error) if explicit_enabled => return Err(error),
            Err(error) => {
                return Err(format!(
                "PawnIO MSR/RAPL path is not auto-enabled because PawnIOLib was not found: {error}"
            )
                .into())
            }
        };
        let module_path = match resolve_module_path(paths) {
            Ok(path) => path,
            Err(error) if explicit_enabled => return Err(error),
            Err(error) => {
                return Err(format!(
                    "PawnIO MSR/RAPL path is not auto-enabled because IntelMSR.bin was not found: {error}"
                )
                .into())
            }
        };
        let module_blob = std::fs::read(&module_path).map_err(|error| {
            format!(
                "Could not read PawnIO RAPL module at {}: {error}",
                module_path.display()
            )
        })?;
        if module_blob.is_empty() {
            return Err(format!("PawnIO RAPL module {} is empty.", module_path.display()).into());
        }

        let library = unsafe { Library::new(&dll_path) }.map_err(|error| {
            format!(
                "Could not load PawnIOLib from {}: {error}",
                dll_path.display()
            )
        })?;

        let open: PawnIoOpen = unsafe { *library.get(b"pawnio_open\0")? };
        let load: PawnIoLoad = unsafe { *library.get(b"pawnio_load\0")? };
        let execute: PawnIoExecute = unsafe { *library.get(b"pawnio_execute\0")? };
        let close: PawnIoClose = unsafe { *library.get(b"pawnio_close\0")? };

        let mut handle: HANDLE = null_mut();
        hresult_to_result(unsafe { open(&mut handle) }, "pawnio_open")?;
        if handle.is_null() {
            return Err("pawnio_open returned a null executor handle.".into());
        }

        if let Err(error) = hresult_to_result(
            unsafe { load(handle, module_blob.as_ptr(), module_blob.len()) },
            "pawnio_load",
        ) {
            unsafe {
                let _ = close(handle);
            }
            return Err(format!(
                "PawnIO opened, but loading {} failed: {error}",
                module_path.display()
            )
            .into());
        }

        Ok(Self {
            module_path,
            _dll_path: dll_path,
            _library: library,
            api: PawnIoApi { execute, close },
            handle,
        })
    }

    pub fn read_rapl_readback(
        &self,
    ) -> Result<Option<RaplReadback>, Box<dyn std::error::Error + Send + Sync>> {
        let unit_raw = self.read_msr(MSR_RAPL_POWER_UNIT)?;
        let units = winring::decode_rapl_units(unit_raw);
        let package_energy_raw = self.read_msr(MSR_PKG_ENERGY_STATUS)?;
        let package_power_limit_raw = self.read_msr(MSR_PKG_POWER_LIMIT)?;
        let package_power_limit = Some(winring::decode_package_power_limit(
            package_power_limit_raw,
            units.power_unit_w,
        ));

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
        let unit_raw = self.read_msr(MSR_RAPL_POWER_UNIT)?;
        let units = winring::decode_rapl_units(unit_raw);
        let before_raw = self.read_msr(MSR_PKG_POWER_LIMIT)?;
        let before = winring::decode_package_power_limit(before_raw, units.power_unit_w);
        if before.locked {
            return Err("CPU package power-limit MSR is locked by firmware.".into());
        }

        let target_raw =
            winring::compose_package_power_limit_write(before_raw, write, units.power_unit_w)?;
        let after_raw = if target_raw != before_raw {
            self.write_msr(MSR_PKG_POWER_LIMIT, target_raw)?;
            self.read_msr(MSR_PKG_POWER_LIMIT)?
        } else {
            before_raw
        };

        let after = winring::decode_package_power_limit(after_raw, units.power_unit_w);
        winring::verify_package_power_limit_write(target_raw, after_raw, write)?;

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

    fn read_msr(&self, register: u32) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.execute_fixed(IOCTL_READ_MSR, &[register as u64], 1)?[0])
    }

    fn write_msr(
        &self,
        register: u32,
        value: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = self.execute_fixed(IOCTL_WRITE_MSR, &[register as u64, value], 0)?;
        Ok(())
    }

    fn execute_fixed(
        &self,
        name: &str,
        input: &[u64],
        output_count: usize,
    ) -> Result<Vec<u64>, Box<dyn std::error::Error + Send + Sync>> {
        let c_name = CString::new(name)?;
        let mut output = vec![0u64; output_count];
        let mut returned = 0usize;
        hresult_to_result(
            unsafe {
                (self.api.execute)(
                    self.handle,
                    c_name.as_ptr(),
                    input.as_ptr(),
                    input.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    &mut returned,
                )
            },
            name,
        )?;

        if returned != output_count {
            return Err(format!(
                "PawnIO function {name} returned {returned} values, expected {output_count}."
            )
            .into());
        }

        Ok(output)
    }
}

impl Drop for PawnIoContext {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                let _ = (self.api.close)(self.handle);
            }
            self.handle = null_mut();
        }
    }
}

fn pawnio_env_flag() -> Option<bool> {
    env::var(PAWNIO_ENABLE_ENV).ok().and_then(|value| {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

fn pawnio_env_disabled() -> bool {
    pawnio_env_flag() == Some(false)
}

fn resolve_dll_path(
    paths: &ServicePaths,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(value) = env::var(PAWNIO_DLL_ENV) {
        let path = PathBuf::from(value.trim());
        if is_existing_file(&path) {
            return Ok(path);
        }

        return Err(format!(
            "{PAWNIO_DLL_ENV} points to {}, but that file does not exist.",
            path.display()
        )
        .into());
    }

    for path in pawnio_dll_candidates(paths) {
        if is_existing_file(&path) {
            return Ok(path);
        }
    }

    Err(format!(
        "{PAWNIO_DLL_ENV} is not set, and PawnIOLib.dll was not found in AeroForge service drivers, Program Files, or System32."
    )
    .into())
}

fn resolve_module_path(
    paths: &ServicePaths,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(value) = env::var(PAWNIO_MODULE_ENV) {
        let path = PathBuf::from(value.trim());
        if is_existing_file(&path) {
            return Ok(path);
        }
        return Err(format!(
            "{PAWNIO_MODULE_ENV} points to {}, but that file does not exist.",
            path.display()
        )
        .into());
    }

    let path = service_driver_dir(paths).join(PAWNIO_DEFAULT_MODULE_NAME);
    if is_existing_file(&path) {
        Ok(path)
    } else {
        Err(format!(
            "PawnIO is enabled, but no RAPL module was found. Set {PAWNIO_MODULE_ENV} or place {PAWNIO_DEFAULT_MODULE_NAME} under {}.",
            path.parent()
                .map(Path::display)
                .map(|display| display.to_string())
                .unwrap_or_else(|| service_driver_dir(paths).display().to_string())
        )
        .into())
    }
}

fn is_existing_file(path: &Path) -> bool {
    path.is_file()
}

fn pawnio_dll_candidates(paths: &ServicePaths) -> Vec<PathBuf> {
    let mut candidates = vec![service_driver_dir(paths).join("PawnIOLib.dll")];

    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("PawnIO")
                .join("PawnIOLib.dll"),
        );
    }
    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(program_files_x86)
                .join("PawnIO")
                .join("PawnIOLib.dll"),
        );
    }
    if let Some(windir) = env::var_os("WINDIR") {
        candidates.push(PathBuf::from(windir).join("System32").join("PawnIOLib.dll"));
    }

    candidates
}

fn service_driver_dir(paths: &ServicePaths) -> PathBuf {
    paths
        .state_dir
        .parent()
        .map(|root| root.join("drivers"))
        .unwrap_or_else(|| paths.state_dir.join("drivers"))
}

fn hresult_to_result(
    value: i32,
    operation: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if value >= 0 {
        Ok(())
    } else {
        Err(format!("{operation} failed with HRESULT 0x{:08X}.", value as u32).into())
    }
}
