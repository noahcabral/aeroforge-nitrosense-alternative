use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    ptr::{null_mut},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use libloading::{Library, Symbol};
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
        TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS, MODULEENTRY32W,
        PROCESSENTRY32W,
    },
};

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::GpuSnapshot,
};
use crate::paths::{write_log_line, ServicePaths};

// ── NVML constants ────────────────────────────────────────────────────────────

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const MAX_REASONABLE_GPU_POWER_W: f32 = 250.0;

// ── Intervals ─────────────────────────────────────────────────────────────────

/// How often to scan running process modules for NVIDIA dGPU DLLs.
/// The scan is CPU-only; no GPU interaction occurs.
const GPU_PROCESS_SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// NVML poll rate while an active GPU session is detected.
const NVIDIA_GPU_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// How long to keep NVML polling after the last active sample.
const NVIDIA_GPU_ACTIVE_COOLDOWN: Duration = Duration::from_secs(15);

// ── NVIDIA dGPU DLL detection ─────────────────────────────────────────────────

/// NVIDIA user-mode driver DLLs (lowercase) that are only present in a process's
/// module list when that process is actively using the NVIDIA dGPU.
/// On Optimus/hybrid systems, apps that use the iGPU load Intel/AMD drivers
/// instead — none of these DLLs appear in their module lists.
///
///  nvwgf2umx.dll — D3D11 / D3D12 UMD
///  nvoglv64.dll  — OpenGL ICD + Vulkan ICD (modern drivers, shared binary)
///  nvcuda.dll    — CUDA runtime
///  nvvk64.dll    — Vulkan ICD (older NVIDIA drivers, rare)
const NVIDIA_DGPU_DLLS: &[&str] = &[
    "nvwgf2umx.dll",
    "nvoglv64.dll",
    "nvcuda.dll",
    "nvvk64.dll",
];

// ── NVML type aliases ─────────────────────────────────────────────────────────

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

// ── Statics ───────────────────────────────────────────────────────────────────

static NVML_API: OnceLock<Option<NvmlApi>> = OnceLock::new();
static GPU_TELEMETRY_CACHE: OnceLock<Arc<Mutex<GpuTelemetryCache>>> = OnceLock::new();

// ── Cache ─────────────────────────────────────────────────────────────────────

struct GpuTelemetryCache {
    last_refresh: Option<Instant>,
    snapshot: GpuSnapshot,
    refresh_in_flight: bool,
    last_error: Option<String>,
    /// Deadline until which NVML is polled at 1 s (GPU confirmed active).
    active_until: Option<Instant>,
    /// When the last process-module scan ran.
    last_process_scan: Option<Instant>,
    /// Tracks last logged active/idle state for transition-only logging.
    last_logged_active: Option<bool>,
}

impl RefreshState for GpuTelemetryCache {
    fn last_refresh(&self) -> Option<Instant> {
        self.last_refresh
    }
    fn set_last_refresh(&mut self, v: Option<Instant>) {
        self.last_refresh = v;
    }
    fn refresh_in_flight(&self) -> bool {
        self.refresh_in_flight
    }
    fn set_refresh_in_flight(&mut self, v: bool) {
        self.refresh_in_flight = v;
    }
    fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
    fn set_last_error(&mut self, v: Option<String>) {
        self.last_error = v;
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

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
        let g = cache.lock().expect("gpu cache lock");
        let in_cooldown = g.active_until.map(|t| now <= t).unwrap_or(false);
        let scan_due = !in_cooldown
            && g.last_process_scan
                .map(|t| t.elapsed() >= GPU_PROCESS_SCAN_INTERVAL)
                .unwrap_or(true);
        (in_cooldown, scan_due)
    };

    // ── Active mode: NVML poll at 1 s ─────────────────────────────────────────
    // GPU was confirmed in use recently; keep polling and extend the cooldown
    // as long as the workload continues.
    if in_cooldown {
        return refresh_cached_value(
            paths,
            "telemetry-nvidia-gpu",
            &cache,
            NVIDIA_GPU_REFRESH_INTERVAL,
            |s| s.last_refresh().is_none(),
            query_nvml_snapshot,
            |s, r| {
                if let Ok(snap) = r {
                    if is_gpu_active(snap) {
                        s.active_until = Some(Instant::now() + NVIDIA_GPU_ACTIVE_COOLDOWN);
                    }
                    s.snapshot = *snap;
                }
            },
            |s| s.snapshot,
        );
    }

    // ── Process scan: detect NVIDIA dGPU usage without touching the GPU ───────
    // Scan every 5 s.  Checks the loaded module list of every running process
    // for NVIDIA dGPU-specific DLLs (D3D, OpenGL, Vulkan, CUDA UMDs).
    // This is a pure CPU operation — the GPU is never contacted.
    if scan_due {
        {
            cache.lock().expect("gpu cache lock").last_process_scan = Some(now);
        }

        if scan_for_nvidia_dgpu_process() {
            // At least one process has the NVIDIA dGPU driver loaded.
            // The GPU is already in D0 because that process woke it.
            // Run NVML now — no additional wake cost.
            match query_nvml_snapshot() {
                Ok(snapshot) => {
                    let active = is_gpu_active(&snapshot);
                    let mut g = cache.lock().expect("gpu cache lock");
                    g.snapshot = snapshot;
                    g.last_refresh = Some(now);
                    if active {
                        g.active_until = Some(now + NVIDIA_GPU_ACTIVE_COOLDOWN);
                    }
                    log_active_transition(paths, &mut g, active);
                    return g.snapshot;
                }
                Err(e) => {
                    let _ = write_log_line(
                        &paths.component_log("telemetry-nvidia-gpu"),
                        "WARN",
                        &format!("NVML query failed after process scan detected dGPU usage: {e}"),
                    );
                }
            }
        } else {
            // No NVIDIA dGPU DLL found in any process: GPU is truly idle.
            // Clear the cached snapshot so the UI shows default values instead
            // of stale data from the last active session.
            let mut g = cache.lock().expect("gpu cache lock");
            g.snapshot = GpuSnapshot::default();
            log_active_transition(paths, &mut g, false);
        }
    }

    // ── Idle: return cached snapshot without touching GPU ─────────────────────
    let snapshot = cache.lock().expect("gpu cache lock").snapshot;
    snapshot
}

// ── Process / module scan ─────────────────────────────────────────────────────

/// Returns true if any running process has loaded an NVIDIA dGPU-specific
/// user-mode driver DLL.  This is a pure CPU-side check; the GPU hardware is
/// never contacted and cannot be woken by this function.
fn scan_for_nvidia_dgpu_process() -> bool {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return false;
    }

    let mut pe: PROCESSENTRY32W = unsafe { zeroed() };
    pe.dwSize = size_of::<PROCESSENTRY32W>() as u32;

    let mut found = false;

    if unsafe { Process32FirstW(snap, &mut pe) } != 0 {
        loop {
            let pid = pe.th32ProcessID;
            // Skip PID 0 (Idle) and PID 4 (System).
            if pid > 4 && process_has_nvidia_dgpu_dll(pid) {
                found = true;
                break;
            }
            if unsafe { Process32NextW(snap, &mut pe) } == 0 {
                break;
            }
        }
    }

    unsafe { CloseHandle(snap) };
    found
}

/// Returns true if the given process has loaded any of the NVIDIA dGPU DLLs.
fn process_has_nvidia_dgpu_dll(pid: u32) -> bool {
    // TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32 covers both 64-bit and 32-bit
    // modules.  The call fails (access denied) for protected/system processes;
    // we treat that as "no NVIDIA DLL" and continue.
    let snap = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    };
    if snap == INVALID_HANDLE_VALUE {
        return false;
    }

    let mut me: MODULEENTRY32W = unsafe { zeroed() };
    me.dwSize = size_of::<MODULEENTRY32W>() as u32;

    let mut found = false;

    if unsafe { Module32FirstW(snap, &mut me) } != 0 {
        loop {
            if module_name_matches(&me.szModule) {
                found = true;
                break;
            }
            if unsafe { Module32NextW(snap, &mut me) } == 0 {
                break;
            }
        }
    }

    unsafe { CloseHandle(snap) };
    found
}

/// Case-insensitive check of a module's short name (szModule) against
/// the NVIDIA dGPU DLL list.  No heap allocation in the fast (no-match) path.
fn module_name_matches(raw: &[u16; 256]) -> bool {
    let len = raw.iter().position(|&c| c == 0).unwrap_or(256);
    let name = String::from_utf16_lossy(&raw[..len]);
    let lower = name.to_ascii_lowercase();
    NVIDIA_DGPU_DLLS.contains(&lower.as_str())
}

// ── GPU activity helpers ──────────────────────────────────────────────────────

fn is_gpu_active(snapshot: &GpuSnapshot) -> bool {
    snapshot.power_draw_w.map(|p| p >= 5.0).unwrap_or(false)
        || snapshot.usage_percent.map(|u| u >= 1).unwrap_or(false)
}

fn log_active_transition(paths: &ServicePaths, cache: &mut GpuTelemetryCache, active: bool) {
    if cache.last_logged_active == Some(active) {
        return;
    }
    cache.last_logged_active = Some(active);
    let msg = if active {
        format!(
            "NVIDIA dGPU active (power={:.1}W usage={}%). Starting 1 s NVML polling.",
            cache.snapshot.power_draw_w.unwrap_or(0.0),
            cache.snapshot.usage_percent.unwrap_or(0),
        )
    } else {
        "No NVIDIA dGPU process detected. GPU free to idle in RTD3/D3cold.".to_string()
    };
    let _ = write_log_line(&paths.component_log("telemetry-nvidia-gpu"), "INFO", &msg);
}

// ── NVML snapshot ─────────────────────────────────────────────────────────────

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
        let init_v2: NvmlInitV2 =
            **library.get::<Symbol<NvmlInitV2>>(b"nvmlInit_v2\0").ok()?;
        let shutdown: NvmlShutdown =
            **library.get::<Symbol<NvmlShutdown>>(b"nvmlShutdown\0").ok()?;
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
            .map(|s| **s);
        let device_get_enforced_power_limit = library
            .get::<Symbol<NvmlDeviceGetEnforcedPowerLimit>>(b"nvmlDeviceGetEnforcedPowerLimit\0")
            .ok()
            .map(|s| **s);
        let device_get_power_management_default_limit = library
            .get::<Symbol<NvmlDeviceGetPowerManagementDefaultLimit>>(
                b"nvmlDeviceGetPowerManagementDefaultLimit\0",
            )
            .ok()
            .map(|s| **s);
        let device_get_power_management_limit_constraints = library
            .get::<Symbol<NvmlDeviceGetPowerManagementLimitConstraints>>(
                b"nvmlDeviceGetPowerManagementLimitConstraints\0",
            )
            .ok()
            .map(|s| **s);

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
    // Per-query init/shutdown: no persistent NVML session, GPU can enter RTD3.
    nvml_call(unsafe { (api.init_v2)() }, "nvmlInit_v2")?;

    let result = (|| {
        let mut device_count = 0u32;
        nvml_call(
            unsafe { (api.device_get_count_v2)(&mut device_count) },
            "nvmlDeviceGetCount_v2",
        )?;
        if device_count == 0 {
            return Err("NVML: zero NVIDIA devices".into());
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

        let mut util = NvmlUtilization { gpu: 0, memory: 0 };
        nvml_call(
            unsafe { (api.device_get_utilization_rates)(device, &mut util) },
            "nvmlDeviceGetUtilizationRates",
        )?;

        let mut mem = NvmlMemory { total: 0, free: 0, used: 0 };
        nvml_call(
            unsafe { (api.device_get_memory_info)(device, &mut mem) },
            "nvmlDeviceGetMemoryInfo",
        )?;

        let memory_usage_percent = if mem.total > 0 {
            Some(
                ((mem.used as f64 / mem.total as f64) * 100.0)
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
            usage_percent: Some(util.gpu.clamp(0, 100) as u8),
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
    let f = api.device_get_power_management_limit_constraints?;
    let mut min = 0u32;
    let mut max = 0u32;
    if unsafe { f(device, &mut min, &mut max) } == NVML_SUCCESS {
        Some((
            sanitize_power_w(milliwatts_to_watts(min)),
            sanitize_power_w(milliwatts_to_watts(max)),
        ))
    } else {
        None
    }
}

fn milliwatts_to_watts(v: u32) -> f32 {
    ((v as f32 / 1000.0) * 100.0).round() / 100.0
}

fn sanitize_power_w(w: f32) -> Option<f32> {
    if w.is_finite() && (0.0..=MAX_REASONABLE_GPU_POWER_W).contains(&w) {
        Some((w * 100.0).round() / 100.0)
    } else {
        None
    }
}

fn nvml_call(code: i32, name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if code == NVML_SUCCESS {
        Ok(())
    } else {
        Err(format!("{name} failed with NVML status {code}").into())
    }
}

// ── nvidia-smi fallback ───────────────────────────────────────────────────────

fn query_nvidia_smi_snapshot() -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let fields = "temperature.gpu,clocks.current.graphics,utilization.gpu,memory.used,memory.total,power.draw,enforced.power.limit,power.default_limit,power.min_limit,power.max_limit";
    let arg = format!("--query-gpu={fields}");

    let out = std::process::Command::new("nvidia-smi")
        .args([arg.as_str(), "--format=csv,noheader,nounits"])
        .output()?;

    if !out.status.success() {
        return Err(format!(
            "nvidia-smi failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }

    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();

    if line.is_empty() {
        return Err("nvidia-smi returned no rows".into());
    }

    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() < 5 {
        return Err(format!("nvidia-smi unexpected field count: {}", f.len()).into());
    }

    let memory_usage_percent = match (f[3].parse::<f64>().ok(), f[4].parse::<f64>().ok()) {
        (Some(used), Some(total)) if total > 0.0 => {
            Some(((used / total) * 100.0).round().clamp(0.0, 100.0) as u8)
        }
        _ => None,
    };

    Ok(GpuSnapshot {
        usage_percent: f[2].parse::<u16>().ok().map(|v| v.clamp(0, 100) as u8),
        memory_usage_percent,
        temp_c: f[0].parse::<u8>().ok(),
        clock_mhz: f[1].parse::<u16>().ok(),
        power_draw_w: parse_optional_watts(f.get(5).copied()),
        power_limit_w: parse_optional_watts(f.get(6).copied()),
        power_default_limit_w: parse_optional_watts(f.get(7).copied()),
        power_min_limit_w: parse_optional_watts(f.get(8).copied()),
        power_max_limit_w: parse_optional_watts(f.get(9).copied()),
    })
}

fn parse_optional_watts(v: Option<&str>) -> Option<f32> {
    let v = v?.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("N/A") || v == "[N/A]" {
        return None;
    }
    v.parse::<f32>().ok().and_then(sanitize_power_w)
}
