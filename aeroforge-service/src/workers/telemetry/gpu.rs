use std::{
    ffi::c_void,
    sync::OnceLock,
    time::{Duration, Instant},
};

use libloading::{Library, Symbol};

use super::models::GpuSnapshot;

const NVML_SUCCESS: i32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const NVML_STARTUP_WARMUP: Duration = Duration::from_secs(60);

type NvmlDevice = *mut c_void;
type NvmlInitV2 = unsafe extern "C" fn() -> i32;
type NvmlShutdown = unsafe extern "C" fn() -> i32;
type NvmlDeviceGetCountV2 = unsafe extern "C" fn(*mut u32) -> i32;
type NvmlDeviceGetHandleByIndexV2 = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
type NvmlDeviceGetTemperature = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
type NvmlDeviceGetClockInfo = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
type NvmlDeviceGetUtilizationRates = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> i32;
type NvmlDeviceGetMemoryInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> i32;

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
}

static NVML_API: OnceLock<Option<NvmlApi>> = OnceLock::new();
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

pub fn read_gpu_snapshot() -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    if PROCESS_START.get_or_init(Instant::now).elapsed() < NVML_STARTUP_WARMUP {
        return Ok(GpuSnapshot::default());
    }

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

        Ok(GpuSnapshot {
            usage_percent: Some(utilization.gpu.clamp(0, 100) as u8),
            memory_usage_percent,
            temp_c: Some(temperature.clamp(0, 255) as u8),
            clock_mhz: Some(clock_mhz.clamp(0, u16::MAX as u32) as u16),
        })
    })();

    let _ = unsafe { (api.shutdown)() };
    result
}

fn nvml_call(code: i32, call_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if code == NVML_SUCCESS {
        Ok(())
    } else {
        Err(format!("{call_name} failed with NVML status {code}").into())
    }
}

fn query_nvidia_smi_snapshot() -> Result<GpuSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=temperature.gpu,clocks.current.graphics,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
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
    })
}
