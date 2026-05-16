use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    ptr::null_mut,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use libloading::{Library, Symbol};
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
        MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        TH32CS_SNAPPROCESS,
    },
};

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::GpuSnapshot,
};
use crate::paths::{write_log_line, ServicePaths};

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const MAX_REASONABLE_GPU_POWER_W: f32 = 250.0;
const GPU_PROCESS_SCAN_INTERVAL: Duration = Duration::from_secs(5);
const NVIDIA_GPU_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const NVIDIA_GPU_ACTIVE_COOLDOWN: Duration = Duration::from_secs(15);
const NVIDIA_DGPU_DLLS: &[&str] = &["nvwgf2umx.dll", "nvoglv64.dll", "nvcuda.dll", "nvvk64.dll"];

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
    active_until: Option<Instant>,
    last_process_scan: Option<Instant>,
    last_logged_active: Option<bool>,
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
                active_until: None,
                last_process_scan: None,
                last_logged_active: None,
            }))
        })
        .clone();

    let now = Instant::now();

    let (in_cooldown, scan_due) = {
        let guard = cache.lock().expect("gpu telemetry cache lock poisoned");
        let in_cooldown = guard
            .active_until
            .map(|deadline| now <= deadline)
            .unwrap_or(false);
        let scan_due = !in_cooldown
            && guard
                .last_process_scan
                .map(|last_scan| last_scan.elapsed() >= GPU_PROCESS_SCAN_INTERVAL)
                .unwrap_or(true);
        (in_cooldown, scan_due)
    };

    if in_cooldown {
        return refresh_cached_value(
            paths,
            "telemetry-nvidia-gpu",
            &cache,
            NVIDIA_GPU_REFRESH_INTERVAL,
            |state| state.last_refresh().is_none(),
            query_nvml_snapshot,
            |state, result| {
                if let Ok(snapshot) = result {
                    if is_gpu_active(snapshot) {
                        state.active_until = Some(Instant::now() + NVIDIA_GPU_ACTIVE_COOLDOWN);
                    }
                    state.snapshot = *snapshot;
                }
            },
            |state| state.snapshot,
        );
    }

    if scan_due {
        {
            cache
                .lock()
                .expect("gpu telemetry cache lock poisoned")
                .last_process_scan = Some(now);
        }

        if scan_for_nvidia_dgpu_process() {
            match query_nvml_snapshot() {
                Ok(snapshot) => {
                    let active = is_gpu_active(&snapshot);
                    let mut guard = cache.lock().expect("gpu telemetry cache lock poisoned");
                    guard.snapshot = snapshot;
                    guard.last_refresh = Some(now);
                    if active {
                        guard.active_until = Some(now + NVIDIA_GPU_ACTIVE_COOLDOWN);
                    }
                    log_active_transition(paths, &mut guard, active);
                    return guard.snapshot;
                }
                Err(error) => {
                    let _ = write_log_line(
                        &paths.component_log("telemetry-nvidia-gpu"),
                        "WARN",
                        &format!(
                            "NVML query failed after process scan detected dGPU usage: {error}"
                        ),
                    );
                }
            }
        } else {
            let mut guard = cache.lock().expect("gpu telemetry cache lock poisoned");
            guard.snapshot = GpuSnapshot::default();
            log_active_transition(paths, &mut guard, false);
        }
    }

    let snapshot = cache
        .lock()
        .expect("gpu telemetry cache lock poisoned")
        .snapshot;
    snapshot
}

fn scan_for_nvidia_dgpu_process() -> bool {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return false;
    }

    let mut process_entry: PROCESSENTRY32W = unsafe { zeroed() };
    process_entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

    let mut found = false;
    if unsafe { Process32FirstW(snapshot, &mut process_entry) } != 0 {
        loop {
            let process_id = process_entry.th32ProcessID;
            if process_id > 4 && process_has_nvidia_dgpu_dll(process_id) {
                found = true;
                break;
            }

            if unsafe { Process32NextW(snapshot, &mut process_entry) } == 0 {
                break;
            }
        }
    }

    unsafe {
        CloseHandle(snapshot);
    }
    found
}

fn process_has_nvidia_dgpu_dll(process_id: u32) -> bool {
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, process_id) };
    if snapshot == INVALID_HANDLE_VALUE {
        return false;
    }

    let mut module_entry: MODULEENTRY32W = unsafe { zeroed() };
    module_entry.dwSize = size_of::<MODULEENTRY32W>() as u32;

    let mut found = false;
    if unsafe { Module32FirstW(snapshot, &mut module_entry) } != 0 {
        loop {
            if module_name_matches(&module_entry.szModule) {
                found = true;
                break;
            }

            if unsafe { Module32NextW(snapshot, &mut module_entry) } == 0 {
                break;
            }
        }
    }

    unsafe {
        CloseHandle(snapshot);
    }
    found
}

fn module_name_matches(raw: &[u16; 256]) -> bool {
    let len = raw.iter().position(|&value| value == 0).unwrap_or(256);
    let name = String::from_utf16_lossy(&raw[..len]);
    let lower = name.to_ascii_lowercase();
    NVIDIA_DGPU_DLLS.contains(&lower.as_str())
}

fn is_gpu_active(snapshot: &GpuSnapshot) -> bool {
    snapshot
        .power_draw_w
        .map(|power| power >= 5.0)
        .unwrap_or(false)
        || snapshot
            .usage_percent
            .map(|usage| usage >= 1)
            .unwrap_or(false)
}

fn log_active_transition(paths: &ServicePaths, cache: &mut GpuTelemetryCache, active: bool) {
    if cache.last_logged_active == Some(active) {
        return;
    }

    cache.last_logged_active = Some(active);
    let message = if active {
        format!(
            "NVIDIA dGPU active (power={:.1}W usage={}%). Starting 1s NVML polling.",
            cache.snapshot.power_draw_w.unwrap_or(0.0),
            cache.snapshot.usage_percent.unwrap_or(0),
        )
    } else {
        "No NVIDIA dGPU process detected. GPU free to idle in RTD3/D3cold.".to_string()
    };
    let _ = write_log_line(
        &paths.component_log("telemetry-nvidia-gpu"),
        "INFO",
        &message,
    );
}

fn query_nvml_snapshot() -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(api) = NVML_API.get_or_init(load_nvml_api).as_ref() {
        if let Ok(snapshot) = read_nvml_snapshot(api) {
            return Ok(snapshot);
        }
    }

    query_nvidia_smi_snapshot()
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

        let mut device: NvmlDevice = null_mut();
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
    if unsafe { function(device, &mut value) } == NVML_SUCCESS {
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
    if unsafe { function(device, &mut min_limit, &mut max_limit) } == NVML_SUCCESS {
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
    let query_fields = "temperature.gpu,clocks.current.graphics,utilization.gpu,memory.used,memory.total,power.draw,enforced.power.limit,power.default_limit,power.min_limit,power.max_limit";
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
        power_draw_w: parse_optional_watts(fields.get(5).copied()),
        power_limit_w: parse_optional_watts(fields.get(6).copied()),
        power_default_limit_w: parse_optional_watts(fields.get(7).copied()),
        power_min_limit_w: parse_optional_watts(fields.get(8).copied()),
        power_max_limit_w: parse_optional_watts(fields.get(9).copied()),
    })
}

fn parse_optional_watts(value: Option<&str>) -> Option<f32> {
    let value = value?.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("N/A") || value == "[N/A]" {
        return None;
    }

    value.parse::<f32>().ok().and_then(sanitize_power_w)
}
