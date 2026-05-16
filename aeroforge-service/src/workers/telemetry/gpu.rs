use std::{
    ffi::c_void,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use libloading::{Library, Symbol};

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::GpuSnapshot,
};
use crate::paths::{write_log_line, ServicePaths};

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;

/// Poll interval while an active GPU session is detected (gaming / compute).
const NVIDIA_GPU_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// How long to keep polling at 1 s after the last active sample before reverting
/// to idle probes. Gives the UI a chance to show the GPU winding down cleanly.
const NVIDIA_GPU_ACTIVE_COOLDOWN: Duration = Duration::from_secs(15);

/// How often to do a single NVML probe while the GPU appears idle.
/// Each probe is a full nvmlInit → query → nvmlShutdown cycle which momentarily
/// wakes the GPU; 30 s gives the driver enough time to re-enter RTD3/D3cold
/// between probes and keeps average power impact negligible.
const NVIDIA_GPU_IDLE_PROBE_INTERVAL: Duration = Duration::from_secs(30);

const MAX_REASONABLE_GPU_POWER_W: f32 = 250.0;

/// Power draw threshold above which the dGPU is considered active.
const GPU_ACTIVE_POWER_W: f32 = 5.0;

/// Core utilisation threshold above which the dGPU is considered active.
const GPU_ACTIVE_USAGE_PERCENT: u8 = 1;

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
    /// Deadline until which the GPU is in "active" mode — NVML is polled every second.
    active_until: Option<Instant>,
    /// When the last idle probe was issued. None means probe immediately on first call.
    last_idle_probe: Option<Instant>,
    /// Tracks the last logged active/idle state so we only log on transitions.
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
                last_idle_probe: None,
                last_logged_active: None,
            }))
        })
        .clone();

    let now = Instant::now();

    let (in_cooldown, probe_due) = {
        let guard = cache.lock().expect("gpu telemetry cache lock poisoned");
        let in_cooldown = guard.active_until.map(|t| now <= t).unwrap_or(false);
        let probe_due = !in_cooldown
            && guard
                .last_idle_probe
                .map(|t| t.elapsed() >= NVIDIA_GPU_IDLE_PROBE_INTERVAL)
                .unwrap_or(true);
        (in_cooldown, probe_due)
    };

    if in_cooldown {
        // Active mode: non-blocking 1 s refresh via background thread.
        // The on_update callback extends active_until whenever the GPU is still busy,
        // so polling continues as long as the workload lasts.
        return refresh_cached_value(
            paths,
            "telemetry-nvidia-gpu",
            &cache,
            NVIDIA_GPU_REFRESH_INTERVAL,
            |state| state.last_refresh().is_none(),
            query_gpu_snapshot,
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

    if probe_due {
        // Idle probe: a single synchronous NVML init → query → shutdown to check
        // whether a game or compute session has started.
        // We record the probe time *before* the query so that even a slow or
        // failed probe still backs off for the full interval.
        {
            let mut guard = cache.lock().expect("gpu telemetry cache lock poisoned");
            guard.last_idle_probe = Some(now);
        }

        match query_gpu_snapshot() {
            Ok(snapshot) => {
                let active = is_gpu_active(&snapshot);
                let mut guard = cache.lock().expect("gpu telemetry cache lock poisoned");
                guard.snapshot = snapshot;
                guard.last_refresh = Some(now);
                if active {
                    guard.active_until = Some(now + NVIDIA_GPU_ACTIVE_COOLDOWN);
                }
                log_activity_transition(paths, &mut guard, active);
                guard.snapshot
            }
            Err(error) => {
                let _ = write_log_line(
                    &paths.component_log("telemetry-nvidia-gpu"),
                    "WARN",
                    &format!("GPU idle probe failed: {error}"),
                );
                GpuSnapshot::default()
            }
        }
    } else {
        // Between idle probes: return the last cached snapshot without touching the GPU.
        // No NVML, no PDH, no dxgkrnl — the dGPU is free to stay in RTD3/D3cold.
        let guard = cache.lock().expect("gpu telemetry cache lock poisoned");
        guard.snapshot
    }
}

/// Returns true when the snapshot suggests the dGPU is running a real workload.
fn is_gpu_active(snapshot: &GpuSnapshot) -> bool {
    snapshot
        .power_draw_w
        .map(|p| p >= GPU_ACTIVE_POWER_W)
        .unwrap_or(false)
        || snapshot
            .usage_percent
            .map(|u| u >= GPU_ACTIVE_USAGE_PERCENT)
            .unwrap_or(false)
}

/// Logs a message only when the active/idle state changes.
fn log_activity_transition(paths: &ServicePaths, cache: &mut GpuTelemetryCache, active: bool) {
    if cache.last_logged_active == Some(active) {
        return;
    }
    cache.last_logged_active = Some(active);
    let message = if active {
        format!(
            "GPU became active (power={:.1}W usage={}%). Switching to {}s polling.",
            cache.snapshot.power_draw_w.unwrap_or(0.0),
            cache.snapshot.usage_percent.unwrap_or(0),
            NVIDIA_GPU_REFRESH_INTERVAL.as_secs(),
        )
    } else {
        format!(
            "GPU appears idle. Switching to {}s idle probes; RTD3/D3cold unrestricted.",
            NVIDIA_GPU_IDLE_PROBE_INTERVAL.as_secs(),
        )
    };
    let _ = write_log_line(&paths.component_log("telemetry-nvidia-gpu"), "INFO", &message);
}

fn query_gpu_snapshot() -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
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
    // Init and shutdown per-query so no persistent NVML session is held.
    // A permanent session would prevent the dGPU from entering RTD3/D3cold
    // even when no game is running.
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
        ) = {
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
