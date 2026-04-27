use std::{
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

extern "system" {
    fn GetSystemTimes(
        lp_idle_time: *mut FILETIME,
        lp_kernel_time: *mut FILETIME,
        lp_user_time: *mut FILETIME,
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
        std::time::Duration::from_millis(333),
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
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "$cores = Get-CimInstance Win32_PerfFormattedData_Counters_ProcessorInformation -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^\\d+,\\d+$' }; if ($cores) { $effective = $cores | ForEach-Object { ([double]$_.ProcessorFrequency) * (([double]$_.PercentProcessorPerformance) / 100.0) } | Measure-Object -Average; [int][math]::Round($effective.Average) } else { $total = Get-CimInstance Win32_PerfFormattedData_Counters_ProcessorInformation -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq '_Total' } | Select-Object -First 1; if ($total) { [int][math]::Round(([double]$total.ProcessorFrequency) * (([double]$total.PercentProcessorPerformance) / 100.0)) } else { Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty CurrentClockSpeed } }",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("PowerShell CPU clock query failed: {stderr}").into());
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(value.parse::<u16>().unwrap_or(0))
}

fn filetime_to_u64(value: FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}
