use std::{
    env,
    ffi::c_void,
    ptr::{null, null_mut},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use libloading::{Library, Symbol};
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::GpuSnapshot,
};
use crate::paths::{write_log_line, ServicePaths};

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const NVIDIA_GPU_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const WINDOWS_GPU_ACTIVITY_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const NVIDIA_GPU_ACTIVE_COOLDOWN: Duration = Duration::from_secs(30);
const MAX_REASONABLE_GPU_POWER_W: f32 = 250.0;
const DISCRETE_GPU_ACTIVE_DEDICATED_BYTES: f64 = 16.0 * 1024.0 * 1024.0;
const NVIDIA_POWER_TELEMETRY_ENV: &str = "AEROFORGE_ENABLE_NVIDIA_TELEMETRY";
const GPU_ADAPTER_DEDICATED_USAGE_COUNTER: &str = r"\GPU Adapter Memory(*)\Dedicated Usage";
const ERROR_SUCCESS: u32 = 0;

type NvmlDevice = *mut c_void;
type NvmlInitV2 = unsafe extern "C" fn() -> i32;
type NvmlShutdown = unsafe extern "C" fn() -> i32;
type NvmlDeviceGetCountV2 = unsafe extern "C" fn(*mut u32) -> i32;
type NvmlDeviceGetHandleByIndexV2 = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
type NvmlDeviceGetTemperature = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
type NvmlDeviceGetClockInfo = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
type NvmlDeviceGetUtilizationRates = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> i32;
type NvmlDeviceGetMemoryInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> i32;
type NvmlDeviceGetPowerUsage = unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32;
type NvmlDeviceGetEnforcedPowerLimit = unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32;
type NvmlDeviceGetPowerManagementDefaultLimit = unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32;
type NvmlDeviceGetPowerManagementLimitConstraints =
    unsafe extern "C" fn(NvmlDevice, *mut u32, *mut u32) -> i32;

#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

struct NvmlApi {
    _library: Library,
    init_v2: NvmlInitV2,
    shutdown: NvmlShutdown,
    device_get_count_v2: NvmlDeviceGetCountV2,
    device_get_handle_by_index_v2: NvmlDeviceGetHandleByIndexV2,
    device_get_temperature: NvmlDeviceGetTemperature,
    device_get_clock_info: NvmlDeviceGetClockInfo,
    device_get_utilization_rates: NvmlDeviceGetUtilizationRates,
    device_get_memory_info: NvmlDeviceGetMemoryInfo,
    device_get_power_usage: Option<NvmlDeviceGetPowerUsage>,
    device_get_enforced_power_limit: Option<NvmlDeviceGetEnforcedPowerLimit>,
    device_get_power_management_default_limit: Option<NvmlDeviceGetPowerManagementDefaultLimit>,
    device_get_power_management_limit_constraints:
        Option<NvmlDeviceGetPowerManagementLimitConstraints>,
}

static NVML_API: OnceLock<Option<NvmlApi>> = OnceLock::new();
static GPU_TELEMETRY_CACHE: OnceLock<Arc<Mutex<GpuTelemetryCache>>> = OnceLock::new();

struct GpuTelemetryCache {
    last_refresh: Option<Instant>,
    snapshot: GpuSnapshot,
    refresh_in_flight: bool,
    last_error: Option<String>,
    last_activity_check: Option<Instant>,
    active_until: Option<Instant>,
    last_gate_error: Option<String>,
    last_gate_allows_nvml: Option<bool>,
}

impl RefreshState for GpuTelemetryCache {
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

pub fn read_gpu_snapshot(paths: &ServicePaths) -> GpuSnapshot {
    let cache = GPU_TELEMETRY_CACHE
        .get_or_init(|| {
            Arc::new(Mutex::new(GpuTelemetryCache {
                last_refresh: None,
                snapshot: GpuSnapshot::default(),
                refresh_in_flight: false,
                last_error: None,
                last_activity_check: None,
                active_until: None,
                last_gate_error: None,
                last_gate_allows_nvml: None,
            }))
        })
        .clone();

    if !gpu_activity_gate_allows_nvml(paths, &cache) {
        return GpuSnapshot::default();
    }

    refresh_cached_value(
        paths,
        "telemetry-nvidia-gpu",
        &cache,
        NVIDIA_GPU_REFRESH_INTERVAL,
        |state| state.last_refresh().is_none(),
        query_gpu_snapshot,
        |state, result| {
            if let Ok(snapshot) = result {
                state.snapshot = *snapshot;
            }
        },
        |state| state.snapshot,
    )
}

fn gpu_activity_gate_allows_nvml(
    paths: &ServicePaths,
    cache: &Arc<Mutex<GpuTelemetryCache>>,
) -> bool {
    let now = Instant::now();
    let should_check = {
        let guard = cache.lock().expect("gpu telemetry cache lock poisoned");
        guard
            .last_activity_check
            .map(|instant| instant.elapsed() >= WINDOWS_GPU_ACTIVITY_REFRESH_INTERVAL)
            .unwrap_or(true)
    };

    if should_check {
        let activity = query_windows_discrete_gpu_activity();
        let mut guard = cache.lock().expect("gpu telemetry cache lock poisoned");
        guard.last_activity_check = Some(now);

        match activity {
            Ok(active) => {
                if active {
                    guard.active_until = Some(now + NVIDIA_GPU_ACTIVE_COOLDOWN);
                }
                let allows_nvml = active || gate_cooldown_active(&guard, now);
                guard.last_gate_error = None;
                log_gate_transition(paths, &mut guard, allows_nvml);
            }
            Err(error) => {
                let detail = format!("Windows GPU activity gate unavailable: {error}");
                if guard.last_gate_error.as_deref() != Some(detail.as_str()) {
                    let _ = write_log_line(
                        &paths.component_log("telemetry-nvidia-gpu"),
                        "WARN",
                        &detail,
                    );
                }
                guard.last_gate_error = Some(detail);
                guard.active_until = Some(now + NVIDIA_GPU_ACTIVE_COOLDOWN);
                log_gate_transition(paths, &mut guard, true);
            }
        }
    }

    let guard = cache.lock().expect("gpu telemetry cache lock poisoned");
    gate_cooldown_active(&guard, now) || guard.last_gate_error.is_some()
}

fn gate_cooldown_active(cache: &GpuTelemetryCache, now: Instant) -> bool {
    cache
        .active_until
        .map(|instant| now <= instant)
        .unwrap_or(false)
}

fn log_gate_transition(paths: &ServicePaths, cache: &mut GpuTelemetryCache, allows_nvml: bool) {
    if cache.last_gate_allows_nvml == Some(allows_nvml) {
        return;
    }

    cache.last_gate_allows_nvml = Some(allows_nvml);
    let message = if allows_nvml {
        "Windows GPU activity gate opened NVML polling."
    } else {
        "Windows GPU activity gate paused NVML polling."
    };
    let _ = write_log_line(
        &paths.component_log("telemetry-nvidia-gpu"),
        "INFO",
        message,
    );
}

fn query_gpu_snapshot() -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(api) = NVML_API.get_or_init(load_nvml_api).as_ref() {
        if let Ok(snapshot) = read_nvml_snapshot(api) {
            return Ok(snapshot);
        }
    }

    query_nvidia_smi_snapshot()
}

fn query_windows_discrete_gpu_activity() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let usages = read_gpu_adapter_dedicated_usage_bytes()?;
    Ok(usages
        .into_iter()
        .any(|usage| usage >= DISCRETE_GPU_ACTIVE_DEDICATED_BYTES))
}

fn read_gpu_adapter_dedicated_usage_bytes(
) -> Result<Vec<f64>, Box<dyn std::error::Error + Send + Sync>> {
    let counter_path = wide_null(GPU_ADAPTER_DEDICATED_USAGE_COUNTER);
    let mut query: PDH_HQUERY = null_mut();
    pdh_call(
        unsafe { PdhOpenQueryW(null(), 0, &mut query) },
        "PdhOpenQueryW",
    )?;
    let query = PdhQuery(query);

    let mut counter: PDH_HCOUNTER = null_mut();
    pdh_call(
        unsafe { PdhAddEnglishCounterW(query.0, counter_path.as_ptr(), 0, &mut counter) },
        "PdhAddEnglishCounterW",
    )?;
    pdh_call(
        unsafe { PdhCollectQueryData(query.0) },
        "PdhCollectQueryData",
    )?;

    let mut buffer_size = 0u32;
    let mut item_count = 0u32;
    let status = unsafe {
        PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut buffer_size,
            &mut item_count,
            null_mut(),
        )
    };
    if status != PDH_MORE_DATA {
        pdh_call(status, "PdhGetFormattedCounterArrayW(size)")?;
    }

    if buffer_size == 0 || item_count == 0 {
        return Ok(Vec::new());
    }

    let item_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    let item_capacity = (buffer_size as usize + item_size - 1) / item_size;
    let mut buffer = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); item_capacity.max(1)];
    pdh_call(
        unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                buffer.as_mut_ptr(),
            )
        },
        "PdhGetFormattedCounterArrayW(data)",
    )?;

    let items = unsafe { std::slice::from_raw_parts(buffer.as_ptr(), item_count as usize) };
    let mut usages = Vec::with_capacity(items.len());
    for item in items {
        if item.FmtValue.CStatus == ERROR_SUCCESS {
            let value = unsafe { item.FmtValue.Anonymous.doubleValue };
            if value.is_finite() && value >= 0.0 {
                usages.push(value);
            }
        }
    }

    Ok(usages)
}

struct PdhQuery(PDH_HQUERY);

impl Drop for PdhQuery {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { PdhCloseQuery(self.0) };
        }
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn pdh_call(status: u32, call_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{call_name} failed with PDH status 0x{status:08X}").into())
    }
}

fn nvidia_power_telemetry_enabled() -> bool {
    env_flag_enabled(NVIDIA_POWER_TELEMETRY_ENV)
}

fn env_flag_enabled(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn load_nvml_api() -> Option<NvmlApi> {
    let library = unsafe { Library::new(r"C:\Windows\System32\nvml.dll").ok()? };

    unsafe {
        let init_v2: NvmlInitV2 = **library.get::<Symbol<NvmlInitV2>>(b"nvmlInit_v2\0").ok()?;
        let shutdown: NvmlShutdown = **library
            .get::<Symbol<NvmlShutdown>>(b"nvmlShutdown\0")
            .ok()?;
        let device_get_count_v2: NvmlDeviceGetCountV2 = **library
            .get::<Symbol<NvmlDeviceGetCountV2>>(b"nvmlDeviceGetCount_v2\0")
            .ok()?;
        let device_get_handle_by_index_v2: NvmlDeviceGetHandleByIndexV2 = **library
            .get::<Symbol<NvmlDeviceGetHandleByIndexV2>>(b"nvmlDeviceGetHandleByIndex_v2\0")
            .ok()?;
        let device_get_temperature: NvmlDeviceGetTemperature = **library
            .get::<Symbol<NvmlDeviceGetTemperature>>(b"nvmlDeviceGetTemperature\0")
            .ok()?;
        let device_get_clock_info: NvmlDeviceGetClockInfo = **library
            .get::<Symbol<NvmlDeviceGetClockInfo>>(b"nvmlDeviceGetClockInfo\0")
            .ok()?;
        let device_get_utilization_rates: NvmlDeviceGetUtilizationRates = **library
            .get::<Symbol<NvmlDeviceGetUtilizationRates>>(b"nvmlDeviceGetUtilizationRates\0")
            .ok()?;
        let device_get_memory_info: NvmlDeviceGetMemoryInfo = **library
            .get::<Symbol<NvmlDeviceGetMemoryInfo>>(b"nvmlDeviceGetMemoryInfo\0")
            .ok()?;
        let device_get_power_usage = library
            .get::<Symbol<NvmlDeviceGetPowerUsage>>(b"nvmlDeviceGetPowerUsage\0")
            .ok()
            .map(|symbol| **symbol);
        let device_get_enforced_power_limit = library
            .get::<Symbol<NvmlDeviceGetEnforcedPowerLimit>>(b"nvmlDeviceGetEnforcedPowerLimit\0")
            .ok()
            .map(|symbol| **symbol);
        let device_get_power_management_default_limit = library
            .get::<Symbol<NvmlDeviceGetPowerManagementDefaultLimit>>(
                b"nvmlDeviceGetPowerManagementDefaultLimit\0",
            )
            .ok()
            .map(|symbol| **symbol);
        let device_get_power_management_limit_constraints = library
            .get::<Symbol<NvmlDeviceGetPowerManagementLimitConstraints>>(
                b"nvmlDeviceGetPowerManagementLimitConstraints\0",
            )
            .ok()
            .map(|symbol| **symbol);

        Some(NvmlApi {
            _library: library,
            init_v2,
            shutdown,
            device_get_count_v2,
            device_get_handle_by_index_v2,
            device_get_temperature,
            device_get_clock_info,
            device_get_utilization_rates,
            device_get_memory_info,
            device_get_power_usage,
            device_get_enforced_power_limit,
            device_get_power_management_default_limit,
            device_get_power_management_limit_constraints,
        })
    }
}

fn read_nvml_snapshot(
    api: &NvmlApi,
) -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    nvml_call(unsafe { (api.init_v2)() }, "nvmlInit_v2")?;

    let result = (|| {
        let mut device_count = 0u32;
        nvml_call(
            unsafe { (api.device_get_count_v2)(&mut device_count) },
            "nvmlDeviceGetCount_v2",
        )?;
        if device_count == 0 {
            return Err("NVML reported zero NVIDIA devices".into());
        }

        let mut device: NvmlDevice = std::ptr::null_mut();
        nvml_call(
            unsafe { (api.device_get_handle_by_index_v2)(0, &mut device) },
            "nvmlDeviceGetHandleByIndex_v2",
        )?;

        let mut temperature = 0u32;
        nvml_call(
            unsafe { (api.device_get_temperature)(device, NVML_TEMPERATURE_GPU, &mut temperature) },
            "nvmlDeviceGetTemperature",
        )?;

        let mut clock_mhz = 0u32;
        nvml_call(
            unsafe { (api.device_get_clock_info)(device, NVML_CLOCK_GRAPHICS, &mut clock_mhz) },
            "nvmlDeviceGetClockInfo",
        )?;

        let mut utilization = NvmlUtilization { gpu: 0, memory: 0 };
        nvml_call(
            unsafe { (api.device_get_utilization_rates)(device, &mut utilization) },
            "nvmlDeviceGetUtilizationRates",
        )?;

        let mut memory = NvmlMemory {
            total: 0,
            free: 0,
            used: 0,
        };
        nvml_call(
            unsafe { (api.device_get_memory_info)(device, &mut memory) },
            "nvmlDeviceGetMemoryInfo",
        )?;

        let memory_usage_percent = if memory.total > 0 {
            Some(
                ((memory.used as f64 / memory.total as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8,
            )
        } else {
            None
        };
        let (
            power_draw_w,
            power_limit_w,
            power_default_limit_w,
            power_min_limit_w,
            power_max_limit_w,
        ) = if nvidia_power_telemetry_enabled() {
            let power_draw_w = read_nvml_power_mw(api.device_get_power_usage, device)
                .map(milliwatts_to_watts)
                .and_then(sanitize_power_w);
            let power_limit_w = read_nvml_power_mw(api.device_get_enforced_power_limit, device)
                .map(milliwatts_to_watts)
                .and_then(sanitize_power_w);
            let power_default_limit_w =
                read_nvml_power_mw(api.device_get_power_management_default_limit, device)
                    .map(milliwatts_to_watts)
                    .and_then(sanitize_power_w);
            let (power_min_limit_w, power_max_limit_w) =
                read_nvml_power_limit_constraints(api, device).unwrap_or((None, None));
            (
                power_draw_w,
                power_limit_w,
                power_default_limit_w,
                power_min_limit_w,
                power_max_limit_w,
            )
        } else {
            (None, None, None, None, None)
        };

        Ok(GpuSnapshot {
            usage_percent: Some(utilization.gpu.clamp(0, 100) as u8),
            memory_usage_percent,
            temp_c: Some(temperature.clamp(0, 255) as u8),
            clock_mhz: Some(clock_mhz.clamp(0, u16::MAX as u32) as u16),
            power_draw_w,
            power_limit_w,
            power_default_limit_w,
            power_min_limit_w,
            power_max_limit_w,
        })
    })();

    let _ = unsafe { (api.shutdown)() };
    result
}

fn read_nvml_power_mw(
    function: Option<unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32>,
    device: NvmlDevice,
) -> Option<u32> {
    let function = function?;
    let mut value = 0u32;
    let status = unsafe { function(device, &mut value) };
    if status == NVML_SUCCESS {
        Some(value)
    } else {
        None
    }
}

fn read_nvml_power_limit_constraints(
    api: &NvmlApi,
    device: NvmlDevice,
) -> Option<(Option<f32>, Option<f32>)> {
    let function = api.device_get_power_management_limit_constraints?;
    let mut min_limit = 0u32;
    let mut max_limit = 0u32;
    let status = unsafe { function(device, &mut min_limit, &mut max_limit) };
    if status == NVML_SUCCESS {
        Some((
            sanitize_power_w(milliwatts_to_watts(min_limit)),
            sanitize_power_w(milliwatts_to_watts(max_limit)),
        ))
    } else {
        None
    }
}

fn milliwatts_to_watts(value: u32) -> f32 {
    ((value as f32 / 1000.0) * 100.0).round() / 100.0
}

fn sanitize_power_w(watts: f32) -> Option<f32> {
    if watts.is_finite() && (0.0..=MAX_REASONABLE_GPU_POWER_W).contains(&watts) {
        Some((watts * 100.0).round() / 100.0)
    } else {
        None
    }
}

fn nvml_call(code: i32, call_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if code == NVML_SUCCESS {
        Ok(())
    } else {
        Err(format!("{call_name} failed with NVML status {code}").into())
    }
}

fn query_nvidia_smi_snapshot() -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let include_power = nvidia_power_telemetry_enabled();
    let query_fields = if include_power {
        "temperature.gpu,clocks.current.graphics,utilization.gpu,memory.used,memory.total,power.draw,enforced.power.limit,power.default_limit,power.min_limit,power.max_limit"
    } else {
        "temperature.gpu,clocks.current.graphics,utilization.gpu,memory.used,memory.total"
    };
    let query_arg = format!("--query-gpu={query_fields}");

    let output = std::process::Command::new("nvidia-smi")
        .args([query_arg.as_str(), "--format=csv,noheader,nounits"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("nvidia-smi GPU query failed: {stderr}").into());
    }

    let line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();

    if line.is_empty() {
        return Err("nvidia-smi returned no GPU rows".into());
    }

    let fields = line
        .split(',')
        .map(|value| value.trim())
        .collect::<Vec<_>>();

    if fields.len() < 5 {
        return Err(format!("unexpected nvidia-smi field count: {}", fields.len()).into());
    }

    let temp_c = fields[0].parse::<u8>().ok();
    let clock_mhz = fields[1].parse::<u16>().ok();
    let usage_percent = fields[2]
        .parse::<u16>()
        .ok()
        .map(|value| value.clamp(0, 100) as u8);
    let memory_used_mib = fields[3].parse::<f64>().ok();
    let memory_total_mib = fields[4].parse::<f64>().ok();
    let memory_usage_percent = match (memory_used_mib, memory_total_mib) {
        (Some(used), Some(total)) if total > 0.0 => {
            Some(((used / total) * 100.0).round().clamp(0.0, 100.0) as u8)
        }
        _ => None,
    };

    Ok(GpuSnapshot {
        usage_percent,
        memory_usage_percent,
        temp_c,
        clock_mhz,
        power_draw_w: include_power
            .then(|| parse_optional_watts(fields.get(5).copied()))
            .flatten(),
        power_limit_w: include_power
            .then(|| parse_optional_watts(fields.get(6).copied()))
            .flatten(),
        power_default_limit_w: include_power
            .then(|| parse_optional_watts(fields.get(7).copied()))
            .flatten(),
        power_min_limit_w: include_power
            .then(|| parse_optional_watts(fields.get(8).copied()))
            .flatten(),
        power_max_limit_w: include_power
            .then(|| parse_optional_watts(fields.get(9).copied()))
            .flatten(),
    })
}

fn parse_optional_watts(value: Option<&str>) -> Option<f32> {
    let value = value?.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("N/A") || value == "[N/A]" {
        return None;
    }

    value.parse::<f32>().ok().and_then(sanitize_power_w)
}
