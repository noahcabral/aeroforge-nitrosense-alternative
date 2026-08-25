use std::{
    ffi::c_void,
    mem::size_of,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use windows_sys::Win32::Foundation::FILETIME;

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::{CpuThermalSnapshot, FirmwareSensorSnapshot, LowLevelSnapshot},
};
use crate::paths::ServicePaths;

static CPU_USAGE_SAMPLER: OnceLock<Mutex<CpuUsageSampler>> = OnceLock::new();
static CPU_CLOCK_CACHE: OnceLock<Arc<Mutex<CpuClockCache>>> = OnceLock::new();
const PROCESSOR_INFORMATION_LEVEL: u32 = 11;
const STATUS_SUCCESS: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessorPowerInformation {
    number: u32,
    max_mhz: u32,
    current_mhz: u32,
    mhz_limit: u32,
    max_idle_state: u32,
    current_idle_state: u32,
}

extern "system" {
    fn GetSystemTimes(
        lp_idle_time: *mut FILETIME,
        lp_kernel_time: *mut FILETIME,
        lp_user_time: *mut FILETIME,
    ) -> i32;

    fn CallNtPowerInformation(
        information_level: u32,
        input_buffer: *mut c_void,
        input_buffer_length: u32,
        output_buffer: *mut c_void,
        output_buffer_length: u32,
    ) -> i32;
}

#[derive(Clone, Copy)]
struct CpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

struct CpuUsageSampler {
    last: Option<CpuTimes>,
}

struct CpuClockCache {
    last_refresh: Option<Instant>,
    value_mhz: u16,
    refresh_in_flight: bool,
    last_error: Option<String>,
}

impl RefreshState for CpuClockCache {
    fn last_refresh(&self) -> Option<Instant> {
        self.last_refresh
    }

    fn set_last_refresh(&mut self, value: Option<Instant>) {
        self.last_refresh = value;
    }

    fn refresh_in_flight(&self) -> bool {
        self.refresh_in_flight
    }

    fn set_refresh_in_flight(&mut self, value: bool) {
        self.refresh_in_flight = value;
    }

    fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn set_last_error(&mut self, value: Option<String>) {
        self.last_error = value;
    }
}

pub fn read_cpu_usage_percent() -> Result<u8, Box<dyn std::error::Error + Send + Sync>> {
    let sampler = CPU_USAGE_SAMPLER.get_or_init(|| Mutex::new(CpuUsageSampler { last: None }));
    let current = read_cpu_times()?;
    let mut guard = sampler.lock().unwrap();

    let usage = if let Some(previous) = guard.last {
        let idle_delta = current.idle.saturating_sub(previous.idle);
        let kernel_delta = current.kernel.saturating_sub(previous.kernel);
        let user_delta = current.user.saturating_sub(previous.user);
        let total_delta = kernel_delta.saturating_add(user_delta);

        if total_delta == 0 {
            0
        } else {
            let busy_delta = total_delta.saturating_sub(idle_delta);
            ((busy_delta as f64 / total_delta as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8
        }
    } else {
        0
    };

    guard.last = Some(current);
    Ok(usage)
}

// made by faxcon
pub fn read_cpu_clock_mhz(paths: &ServicePaths) -> u16 {
    let cache = CPU_CLOCK_CACHE
        .get_or_init(|| {
            Arc::new(Mutex::new(CpuClockCache {
                last_refresh: None,
                value_mhz: 0,
                refresh_in_flight: false,
                last_error: None,
            }))
        })
        .clone();

    let value = refresh_cached_value(
        paths,
        "telemetry-cpu-clock",
        &cache,
        std::time::Duration::from_secs(10),
        |state| state.last_refresh().is_none(),
        query_cpu_clock_mhz,
        |state, result| {
            if let Ok(value) = result {
                state.value_mhz = *value;
            }
        },
        |state| state.value_mhz,
    );

    if value == 0 {
        2646
    } else {
        value
    }
}

pub fn build_cpu_thermal_snapshot(
    low_level: &LowLevelSnapshot,
    firmware: &FirmwareSensorSnapshot,
) -> CpuThermalSnapshot {
    CpuThermalSnapshot {
        average_temp_c: low_level
            .average_core_temp_c
            .or(low_level.package_temp_c)
            .or(firmware.cpu_temp_c)
            .or(firmware.thermal_zone_temp_c),
        lowest_core_temp_c: low_level.lowest_core_temp_c,
        highest_core_temp_c: low_level.highest_core_temp_c,
    }
}

fn read_cpu_times() -> Result<CpuTimes, Box<dyn std::error::Error + Send + Sync>> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();

    let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    Ok(CpuTimes {
        idle: filetime_to_u64(idle),
        kernel: filetime_to_u64(kernel),
        user: filetime_to_u64(user),
    })
}

fn query_cpu_clock_mhz() -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    let processor_count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let mut processors = vec![ProcessorPowerInformation::default(); processor_count];
    let status = unsafe {
        CallNtPowerInformation(
            PROCESSOR_INFORMATION_LEVEL,
            std::ptr::null_mut(),
            0,
            processors.as_mut_ptr().cast(),
            (processors.len() * size_of::<ProcessorPowerInformation>()) as u32,
        )
    };

    if status != STATUS_SUCCESS {
        return Err(
            format!("CallNtPowerInformation ProcessorInformation failed: {status:#x}").into(),
        );
    }

    let mut total_mhz = 0u64;
    let mut sample_count = 0u64;
    for processor in processors {
        if processor.current_mhz > 0 {
            total_mhz += processor.current_mhz as u64;
            sample_count += 1;
        }
    }

    if sample_count == 0 {
        return Err("CallNtPowerInformation returned no CPU clock samples".into());
    }

    Ok((total_mhz / sample_count).min(u16::MAX as u64) as u16)
}

fn filetime_to_u64(value: FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}
