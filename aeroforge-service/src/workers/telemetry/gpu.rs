use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use libloading::{Library, Symbol};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceRegistryPropertyW, DIGCF_PRESENT, SP_DEVINFO_DATA, SPDRP_HARDWAREID,
    },
    Foundation::INVALID_HANDLE_VALUE,
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

// ── Polling intervals ─────────────────────────────────────────────────────────

/// NVML poll rate while the GPU is awake and in active use.
const NVIDIA_GPU_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Fallback probe interval used when the NVIDIA device node cannot be located
/// via SetupAPI (e.g. exotic driver state).  In normal operation the PnP
/// D-state check is used instead and this constant is never reached.
const NVIDIA_GPU_FALLBACK_PROBE_INTERVAL: Duration = Duration::from_secs(30);

// ── PnP / CM constants ────────────────────────────────────────────────────────

/// CM_POWER_DATA.PD_MostRecentPowerState == 1 means PowerDeviceD0 (fully on).
const POWER_DEVICE_D0: u32 = 1;

/// Raw GUID layout — identical to Windows GUID / DEVPROPKEY.fmtid.
/// Defined manually because the windows-sys GUID path varies across versions.
#[repr(C)]
#[derive(Clone, Copy)]
struct RawGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

/// DEVPKEY_Device_PowerData  {83DA6326-97A6-4088-9453-A1923F573B29}, 1
#[repr(C)]
struct RawDevpropkey {
    fmtid: RawGuid,
    pid: u32,
}

const DEVPKEY_DEVICE_POWER_DATA: RawDevpropkey = RawDevpropkey {
    fmtid: RawGuid {
        data1: 0x83DA_6326,
        data2: 0x97A6,
        data3: 0x4088,
        data4: [0x94, 0x53, 0xA1, 0x92, 0x3F, 0x57, 0x3B, 0x29],
    },
    pid: 1,
};

/// GUID_DEVCLASS_DISPLAY  {4D36E968-E325-11CE-BFC1-08002BE10318}
const GUID_DEVCLASS_DISPLAY: RawGuid = RawGuid {
    data1: 0x4D36_E968,
    data2: 0xE325,
    data3: 0x11CE,
    data4: [0xBF, 0xC1, 0x08, 0x00, 0x2B, 0xE1, 0x03, 0x18],
};

/// NVIDIA PCI vendor ID in hardware ID strings ("VEN_10DE").
const NVIDIA_VEN_ID: &str = "VEN_10DE";

/// CR_SUCCESS return value from CM_ functions.
const CR_SUCCESS: u32 = 0;

// ── CM_POWER_DATA layout ──────────────────────────────────────────────────────

/// Manually defined because windows-sys does not expose CM_POWER_DATA.
/// POWER_SYSTEM_MAXIMUM = 7 (PowerSystemMaximum enum value).
#[repr(C)]
struct CmPowerData {
    pd_size: u32,
    /// DEVICE_POWER_STATE: 1=D0, 2=D1, 3=D2, 4=D3 (hot or cold).
    pd_most_recent_power_state: u32,
    pd_capabilities: u32,
    pd_d1_latency: u32,
    pd_d2_latency: u32,
    pd_d3_latency: u32,
    pd_power_state_mapping: [u32; 7],
    pd_deepest_system_wake: u32,
}

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

/// `CM_Get_DevNode_PropertyW` function pointer loaded from cfgmgr32.dll.
/// Returns CR_SUCCESS (0) on success.
type CmGetDevNodePropertyW = unsafe extern "system" fn(
    dn_dev_inst: u32,
    property_key: *const RawDevpropkey,
    property_type: *mut u32,
    property_buffer: *mut u8,
    property_buffer_size: *mut u32,
    ul_flags: u32,
) -> u32;

static CM_GET_DEV_NODE_PROPERTY_W: OnceLock<Option<CmGetDevNodePropertyW>> = OnceLock::new();

static NVML_API: OnceLock<Option<NvmlApi>> = OnceLock::new();
static GPU_TELEMETRY_CACHE: OnceLock<Arc<Mutex<GpuTelemetryCache>>> = OnceLock::new();

/// Cached NVIDIA GPU device instance handle (devinst).
/// None  → no NVIDIA display adapter found via SetupAPI.
/// Some  → devinst is valid for CM_Get_DevNode_PropertyW queries.
static NVIDIA_GPU_DEVINST: OnceLock<Option<u32>> = OnceLock::new();

// ── Cache ─────────────────────────────────────────────────────────────────────

struct GpuTelemetryCache {
    last_refresh: Option<Instant>,
    snapshot: GpuSnapshot,
    refresh_in_flight: bool,
    last_error: Option<String>,
    /// Used only when PnP detection is unavailable (fallback probe path).
    last_fallback_probe: Option<Instant>,
    /// Tracks last logged D0/D3 state to avoid log spam on every tick.
    last_logged_d0: Option<bool>,
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

// ── Public entry point ────────────────────────────────────────────────────────

pub fn read_gpu_snapshot(paths: &ServicePaths) -> GpuSnapshot {
    let cache = GPU_TELEMETRY_CACHE
        .get_or_init(|| {
            Arc::new(Mutex::new(GpuTelemetryCache {
                last_refresh: None,
                snapshot: GpuSnapshot::default(),
                refresh_in_flight: false,
                last_error: None,
                last_fallback_probe: None,
                last_logged_d0: None,
            }))
        })
        .clone();

    let should_poll = match NVIDIA_GPU_DEVINST.get_or_init(find_nvidia_gpu_devinst) {
        Some(devinst) => {
            // Primary path: ask the PnP manager for the GPU's current D-state.
            // This reads from ntoskrnl's device database — no GPU driver contact,
            // no dxgkrnl interaction, the dGPU stays in RTD3/D3cold if it is there.
            let d0 = query_gpu_d0_from_pnp(*devinst);
            log_d0_transition(paths, &cache, d0);
            d0
        }
        None => {
            // Fallback: PnP detection failed (unusual driver/hardware state).
            // Fall back to a 30 s periodic probe so the UI is not permanently blank.
            let now = Instant::now();
            let probe_due = {
                let guard = cache.lock().expect("gpu telemetry cache lock poisoned");
                guard
                    .last_fallback_probe
                    .map(|t| t.elapsed() >= NVIDIA_GPU_FALLBACK_PROBE_INTERVAL)
                    .unwrap_or(true)
            };
            if probe_due {
                let mut guard = cache.lock().expect("gpu telemetry cache lock poisoned");
                guard.last_fallback_probe = Some(now);
            }
            probe_due
        }
    };

    if !should_poll {
        // GPU is in D3/D3cold — return the last cached snapshot without touching
        // NVML, nvidia-smi, PDH, dxgkrnl, or any GPU-adjacent subsystem.
        return cache
            .lock()
            .expect("gpu telemetry cache lock poisoned")
            .snapshot;
    }

    // GPU is in D0 (already awake, woken by a game or app) — or we're in
    // fallback probe mode.  Run NVML now; the GPU is already paying the wake
    // cost so our query adds nothing to power consumption.
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

// ── PnP power state helpers ───────────────────────────────────────────────────

/// Loads `CM_Get_DevNode_PropertyW` from cfgmgr32.dll once and caches the pointer.
fn load_cm_get_dev_node_property_w() -> Option<CmGetDevNodePropertyW> {
    // cfgmgr32.dll is always present on Windows; it is already loaded by the
    // process (via setupapi.dll), so this does not add DLL load overhead.
    let lib = unsafe { Library::new("cfgmgr32.dll").ok()? };
    let func: Symbol<CmGetDevNodePropertyW> =
        unsafe { lib.get(b"CM_Get_DevNode_PropertyW\0").ok()? };
    let ptr = *func;
    // Intentionally leak the Library so the function pointer stays valid.
    std::mem::forget(lib);
    Some(ptr)
}

/// Returns true if the NVIDIA dGPU's PnP power state is D0 (fully on).
/// Reads from the Windows PnP manager database; does NOT contact the GPU driver
/// or dxgkrnl and therefore cannot wake the device from RTD3/D3cold.
fn query_gpu_d0_from_pnp(devinst: u32) -> bool {
    let Some(cm_fn) = *CM_GET_DEV_NODE_PROPERTY_W
        .get_or_init(load_cm_get_dev_node_property_w)
    else {
        return false;
    };

    let mut property_type = 0u32;
    let mut buffer = [0u8; size_of::<CmPowerData>()];
    let mut buffer_size = buffer.len() as u32;

    let result = unsafe {
        cm_fn(
            devinst,
            &DEVPKEY_DEVICE_POWER_DATA,
            &mut property_type,
            buffer.as_mut_ptr(),
            &mut buffer_size,
            0,
        )
    };

    if result != CR_SUCCESS || buffer_size < size_of::<CmPowerData>() as u32 {
        // Query failed — assume the GPU is sleeping so we do not inadvertently
        // wake it.  The UI will show stale (default) data until next D0 event.
        return false;
    }

    let power_data = unsafe { &*(buffer.as_ptr() as *const CmPowerData) };
    power_data.pd_most_recent_power_state == POWER_DEVICE_D0
}

/// Finds the device instance (devinst) of the first NVIDIA display adapter.
/// Called once via OnceLock; result is cached for the service lifetime.
fn find_nvidia_gpu_devinst() -> Option<u32> {
    // SetupDiGetClassDevsW expects a pointer to a Windows GUID.
    // RawGuid has the identical memory layout, so the cast is safe.
    let devinfo = unsafe {
        SetupDiGetClassDevsW(
            &GUID_DEVCLASS_DISPLAY as *const RawGuid as *const _,
            null(),
            null_mut(),
            DIGCF_PRESENT,
        )
    };

    if devinfo == INVALID_HANDLE_VALUE as isize {
        return None;
    }

    let mut index = 0u32;
    let mut found_devinst: Option<u32> = None;

    loop {
        let mut devinfo_data: SP_DEVINFO_DATA = unsafe { zeroed() };
        devinfo_data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;

        let ok = unsafe { SetupDiEnumDeviceInfo(devinfo, index, &mut devinfo_data) };
        if ok == 0 {
            // ERROR_NO_MORE_ITEMS or genuine error — either way, stop.
            break;
        }

        if is_nvidia_device(devinfo, &devinfo_data) {
            found_devinst = Some(devinfo_data.DevInst);
            break;
        }

        index += 1;
    }

    unsafe { SetupDiDestroyDeviceInfoList(devinfo) };
    found_devinst
}

/// Returns true when the device's hardware ID string contains the NVIDIA
/// PCI vendor ID (VEN_10DE).
fn is_nvidia_device(devinfo: isize, devinfo_data: &SP_DEVINFO_DATA) -> bool {
    let mut buffer = vec![0u16; 512];
    let mut required_size = 0u32;

    let ok = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            devinfo,
            devinfo_data,
            SPDRP_HARDWAREID,
            null_mut(),
            buffer.as_mut_ptr() as *mut u8,
            (buffer.len() * size_of::<u16>()) as u32,
            &mut required_size,
        )
    };

    if ok == 0 {
        return false;
    }

    // Hardware ID is a REG_MULTI_SZ: multiple null-separated strings ending
    // with a double null.  Check all strings for the NVIDIA vendor ID.
    let words = required_size as usize / size_of::<u16>();
    let data = &buffer[..words.min(buffer.len())];
    let mut start = 0;
    while start < data.len() {
        let end = data[start..]
            .iter()
            .position(|&c| c == 0)
            .map(|p| start + p)
            .unwrap_or(data.len());
        if end == start {
            break; // double-null terminator
        }
        let segment = String::from_utf16_lossy(&data[start..end]).to_uppercase();
        if segment.contains(NVIDIA_VEN_ID) {
            return true;
        }
        start = end + 1;
    }

    false
}

/// Logs a message when the GPU transitions between D0 and D3 states.
fn log_d0_transition(paths: &ServicePaths, cache: &Arc<Mutex<GpuTelemetryCache>>, d0: bool) {
    let mut guard = cache.lock().expect("gpu telemetry cache lock poisoned");
    if guard.last_logged_d0 == Some(d0) {
        return;
    }
    guard.last_logged_d0 = Some(d0);
    let message = if d0 {
        "GPU entered D0 (woken by another process). Starting NVML polling.".to_string()
    } else {
        "GPU entered D3/D3cold. NVML polling suspended; RTD3 unrestricted.".to_string()
    };
    drop(guard);
    let _ = write_log_line(&paths.component_log("telemetry-nvidia-gpu"), "INFO", &message);
}

// ── NVML snapshot ─────────────────────────────────────────────────────────────

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
    // A permanent session would prevent the dGPU from entering RTD3/D3cold.
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

// ── nvidia-smi fallback ───────────────────────────────────────────────────────

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
