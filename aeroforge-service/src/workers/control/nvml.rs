use std::{
    ffi::{c_char, c_void},
    path::Path,
};

use libloading::Library;

use crate::paths::{write_log_line, ServicePaths};

use super::models::GpuTuningState;

const NVML_SUCCESS: i32 = 0;
const NVML_ERROR_NOT_SUPPORTED: i32 = 3;
const NVML_ERROR_NO_PERMISSION: i32 = 4;
const NVML_ERROR_GPU_IS_LOST: i32 = 15;

const NVML_CLOCK_GRAPHICS: u32 = 0;
const NVML_CLOCK_MEM: u32 = 2;
const NVML_PSTATE_0: u32 = 0;
const NVML_CLOCK_OFFSET_V1: u32 = 1 << 24;

type NvmlDevice = *mut c_void;
type NvmlInitV2 = unsafe extern "C" fn() -> i32;
type NvmlShutdown = unsafe extern "C" fn() -> i32;
type NvmlDeviceGetCountV2 = unsafe extern "C" fn(*mut u32) -> i32;
type NvmlDeviceGetHandleByIndexV2 = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
type NvmlDeviceGetName = unsafe extern "C" fn(NvmlDevice, *mut c_char, u32) -> i32;
type NvmlDeviceGetClockOffsets = unsafe extern "C" fn(NvmlDevice, *mut NvmlClockOffsetV1) -> i32;
type NvmlDeviceSetClockOffsets = unsafe extern "C" fn(NvmlDevice, *mut NvmlClockOffsetV1) -> i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvmlClockOffsetV1 {
    version: u32,
    clock_type: u32,
    pstate: u32,
    clock_offset_mhz: i32,
    min_clock_offset_mhz: i32,
    max_clock_offset_mhz: i32,
}

pub struct NvmlGpuTuningReport {
    pub gpu_name: String,
    pub applied_core_clock_mhz: Option<i16>,
    pub applied_memory_clock_mhz: Option<i16>,
    pub unsupported_fields: Vec<&'static str>,
    pub detail: String,
}

struct DomainResult {
    applied_mhz: Option<i16>,
    unsupported: bool,
    detail: String,
}

struct NvmlApi {
    _library: Library,
    init_v2: NvmlInitV2,
    shutdown: NvmlShutdown,
    device_get_count_v2: NvmlDeviceGetCountV2,
    device_get_handle_by_index_v2: NvmlDeviceGetHandleByIndexV2,
    device_get_name: NvmlDeviceGetName,
    device_get_clock_offsets: NvmlDeviceGetClockOffsets,
    device_set_clock_offsets: NvmlDeviceSetClockOffsets,
}

pub fn apply_gpu_tuning(paths: &ServicePaths, tuning: &GpuTuningState) -> NvmlGpuTuningReport {
    let core_log_path = paths.component_log("control-gpu-core");
    let memory_log_path = paths.component_log("control-gpu-memory");

    match NvmlApi::load()
        .and_then(|api| api.apply_gpu_tuning(&core_log_path, &memory_log_path, tuning))
    {
        Ok(report) => report,
        Err(error) => {
            let detail = format!("NVML GPU tuning path failed: {error}");
            let _ = write_log_line(&core_log_path, "ERROR", &detail);
            let _ = write_log_line(&memory_log_path, "ERROR", &detail);
            NvmlGpuTuningReport {
                gpu_name: "NVIDIA GPU".into(),
                applied_core_clock_mhz: None,
                applied_memory_clock_mhz: None,
                unsupported_fields: vec![
                    "coreClock",
                    "memoryClock",
                    "voltageOffset",
                    "powerLimit",
                    "tempLimit",
                ],
                detail,
            }
        }
    }
}

impl NvmlApi {
    fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let library = unsafe { Library::new(r"C:\Windows\System32\nvml.dll") }?;

        unsafe {
            Ok(Self {
                init_v2: *library.get::<NvmlInitV2>(b"nvmlInit_v2\0")?,
                shutdown: *library.get::<NvmlShutdown>(b"nvmlShutdown\0")?,
                device_get_count_v2: *library
                    .get::<NvmlDeviceGetCountV2>(b"nvmlDeviceGetCount_v2\0")?,
                device_get_handle_by_index_v2: *library
                    .get::<NvmlDeviceGetHandleByIndexV2>(b"nvmlDeviceGetHandleByIndex_v2\0")?,
                device_get_name: *library.get::<NvmlDeviceGetName>(b"nvmlDeviceGetName\0")?,
                device_get_clock_offsets: *library
                    .get::<NvmlDeviceGetClockOffsets>(b"nvmlDeviceGetClockOffsets\0")?,
                device_set_clock_offsets: *library
                    .get::<NvmlDeviceSetClockOffsets>(b"nvmlDeviceSetClockOffsets\0")?,
                _library: library,
            })
        }
    }

    fn apply_gpu_tuning(
        &self,
        core_log_path: &Path,
        memory_log_path: &Path,
        tuning: &GpuTuningState,
    ) -> Result<NvmlGpuTuningReport, Box<dyn std::error::Error + Send + Sync>> {
        self.require_ok(unsafe { (self.init_v2)() }, "nvmlInit_v2")?;

        let result = (|| {
            let device = self.first_device()?;
            let gpu_name = self.read_gpu_name(device);

            let core_result = self.apply_domain(
                core_log_path,
                device,
                &gpu_name,
                "core",
                "coreClock",
                NVML_CLOCK_GRAPHICS,
                i32::from(tuning.core_clock_mhz),
            )?;
            let memory_result = self.apply_domain(
                memory_log_path,
                device,
                &gpu_name,
                "memory",
                "memoryClock",
                NVML_CLOCK_MEM,
                i32::from(tuning.memory_clock_mhz),
            )?;

            let mut applied_domains = Vec::new();
            if let Some(value) = core_result.applied_mhz {
                applied_domains.push(format!("core {:+} MHz", value));
            }
            if let Some(value) = memory_result.applied_mhz {
                applied_domains.push(format!("memory {:+} MHz", value));
            }

            let mut detail_parts = Vec::new();
            if applied_domains.is_empty() {
                detail_parts.push(format!(
                    "{gpu_name} did not expose any live writable GPU tuning domains for the requested tuning."
                ));
            } else {
                detail_parts.push(format!(
                    "Applied {} on {gpu_name}.",
                    applied_domains.join(" and ")
                ));
            }

            let mut unsupported_fields = vec!["voltageOffset", "powerLimit", "tempLimit"];
            if core_result.unsupported {
                unsupported_fields.push("coreClock");
            }
            if memory_result.unsupported {
                unsupported_fields.push("memoryClock");
            }

            detail_parts.push(core_result.detail);
            detail_parts.push(memory_result.detail);

            Ok(NvmlGpuTuningReport {
                gpu_name,
                applied_core_clock_mhz: core_result.applied_mhz,
                applied_memory_clock_mhz: memory_result.applied_mhz,
                unsupported_fields,
                detail: detail_parts.join(" "),
            })
        })();

        let shutdown_status = unsafe { (self.shutdown)() };
        if shutdown_status != NVML_SUCCESS {
            let detail = format!(
                "nvmlShutdown returned {}.",
                nvml_status_name(shutdown_status)
            );
            let _ = write_log_line(core_log_path, "WARN", &detail);
            let _ = write_log_line(memory_log_path, "WARN", &detail);
        }

        result
    }

    fn apply_domain(
        &self,
        log_path: &Path,
        device: NvmlDevice,
        gpu_name: &str,
        domain_label: &str,
        field_name: &'static str,
        clock_type: u32,
        requested: i32,
    ) -> Result<DomainResult, Box<dyn std::error::Error + Send + Sync>> {
        let current_info = match self.read_clock_offsets(device, clock_type) {
            Ok(info) => info,
            Err(error) => {
                let detail = format!("NVML {domain_label} path unavailable: {error}");
                write_log_line(log_path, "WARN", &detail)?;
                return Ok(DomainResult {
                    applied_mhz: None,
                    unsupported: true,
                    detail,
                });
            }
        };

        let clamped = requested.clamp(
            current_info.min_clock_offset_mhz,
            current_info.max_clock_offset_mhz,
        );

        write_log_line(
            log_path,
            "INFO",
            &format!(
                "Applying NVML {domain_label} offset {:+} MHz on {gpu_name}. Current {:+} MHz, supported range {:+}..{:+} MHz.",
                requested,
                current_info.clock_offset_mhz,
                current_info.min_clock_offset_mhz,
                current_info.max_clock_offset_mhz
            ),
        )?;

        let set_status = self.set_clock_offset(device, clock_type, clamped);
        if set_status == NVML_SUCCESS {
            let applied_info = self
                .read_clock_offsets(device, clock_type)
                .unwrap_or_else(|_| {
                    let mut fallback = current_info;
                    fallback.clock_offset_mhz = clamped;
                    fallback
                });

            let detail = format!(
                "NVML {domain_label} path applied {:+} MHz on {gpu_name} (requested {:+} MHz, range {:+}..{:+} MHz).",
                applied_info.clock_offset_mhz,
                requested,
                current_info.min_clock_offset_mhz,
                current_info.max_clock_offset_mhz
            );
            write_log_line(log_path, "INFO", &detail)?;
            return Ok(DomainResult {
                applied_mhz: Some(applied_info.clock_offset_mhz as i16),
                unsupported: false,
                detail,
            });
        }

        let detail = format!(
            "NVML {domain_label} path returned {} while applying {:+} MHz on {gpu_name}.",
            nvml_status_name(set_status),
            requested
        );
        write_log_line(log_path, "WARN", &detail)?;

        Ok(DomainResult {
            applied_mhz: None,
            unsupported: matches!(
                set_status,
                NVML_ERROR_NOT_SUPPORTED | NVML_ERROR_NO_PERMISSION | NVML_ERROR_GPU_IS_LOST
            ) || field_name == "coreClock"
                || field_name == "memoryClock",
            detail,
        })
    }

    fn read_clock_offsets(
        &self,
        device: NvmlDevice,
        clock_type: u32,
    ) -> Result<NvmlClockOffsetV1, Box<dyn std::error::Error + Send + Sync>> {
        let mut info = NvmlClockOffsetV1 {
            version: std::mem::size_of::<NvmlClockOffsetV1>() as u32 | NVML_CLOCK_OFFSET_V1,
            clock_type,
            pstate: NVML_PSTATE_0,
            ..Default::default()
        };

        self.require_ok(
            unsafe { (self.device_get_clock_offsets)(device, &mut info) },
            "nvmlDeviceGetClockOffsets",
        )?;
        Ok(info)
    }

    fn set_clock_offset(&self, device: NvmlDevice, clock_type: u32, value: i32) -> i32 {
        let mut info = NvmlClockOffsetV1 {
            version: std::mem::size_of::<NvmlClockOffsetV1>() as u32 | NVML_CLOCK_OFFSET_V1,
            clock_type,
            pstate: NVML_PSTATE_0,
            clock_offset_mhz: value,
            ..Default::default()
        };
        unsafe { (self.device_set_clock_offsets)(device, &mut info) }
    }

    fn first_device(&self) -> Result<NvmlDevice, Box<dyn std::error::Error + Send + Sync>> {
        let mut device_count = 0u32;
        self.require_ok(
            unsafe { (self.device_get_count_v2)(&mut device_count) },
            "nvmlDeviceGetCount_v2",
        )?;
        if device_count == 0 {
            return Err("NVML reported zero NVIDIA devices.".into());
        }

        let mut device: NvmlDevice = std::ptr::null_mut();
        self.require_ok(
            unsafe { (self.device_get_handle_by_index_v2)(0, &mut device) },
            "nvmlDeviceGetHandleByIndex_v2",
        )?;
        Ok(device)
    }

    fn read_gpu_name(&self, device: NvmlDevice) -> String {
        let mut buffer = [0_i8; 96];
        let status =
            unsafe { (self.device_get_name)(device, buffer.as_mut_ptr(), buffer.len() as u32) };
        if status != NVML_SUCCESS {
            return "NVIDIA GPU".into();
        }

        let bytes = buffer
            .iter()
            .take_while(|value| **value != 0)
            .map(|value| *value as u8)
            .collect::<Vec<_>>();
        String::from_utf8_lossy(&bytes).trim().to_string()
    }

    fn require_ok(
        &self,
        status: i32,
        operation: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if status == NVML_SUCCESS {
            return Ok(());
        }

        Err(format!("{operation} failed: {}", nvml_status_name(status)).into())
    }
}

fn nvml_status_name(code: i32) -> &'static str {
    match code {
        0 => "0 (NVML_SUCCESS)",
        1 => "1 (NVML_ERROR_UNINITIALIZED)",
        2 => "2 (NVML_ERROR_INVALID_ARGUMENT)",
        3 => "3 (NVML_ERROR_NOT_SUPPORTED)",
        4 => "4 (NVML_ERROR_NO_PERMISSION)",
        5 => "5 (NVML_ERROR_ALREADY_INITIALIZED)",
        6 => "6 (NVML_ERROR_NOT_FOUND)",
        7 => "7 (NVML_ERROR_INSUFFICIENT_SIZE)",
        8 => "8 (NVML_ERROR_INSUFFICIENT_POWER)",
        9 => "9 (NVML_ERROR_DRIVER_NOT_LOADED)",
        10 => "10 (NVML_ERROR_TIMEOUT)",
        11 => "11 (NVML_ERROR_IRQ_ISSUE)",
        12 => "12 (NVML_ERROR_LIBRARY_NOT_FOUND)",
        13 => "13 (NVML_ERROR_FUNCTION_NOT_FOUND)",
        14 => "14 (NVML_ERROR_CORRUPTED_INFOROM)",
        15 => "15 (NVML_ERROR_GPU_IS_LOST)",
        16 => "16 (NVML_ERROR_RESET_REQUIRED)",
        17 => "17 (NVML_ERROR_OPERATING_SYSTEM)",
        18 => "18 (NVML_ERROR_LIB_RM_VERSION_MISMATCH)",
        19 => "19 (NVML_ERROR_IN_USE)",
        20 => "20 (NVML_ERROR_MEMORY)",
        21 => "21 (NVML_ERROR_NO_DATA)",
        22 => "22 (NVML_ERROR_VGPU_ECC_NOT_SUPPORTED)",
        23 => "23 (NVML_ERROR_INSUFFICIENT_RESOURCES)",
        25 => "25 (NVML_ERROR_ARGUMENT_VERSION_MISMATCH)",
        _ => "999 (NVML_ERROR_UNKNOWN)",
    }
}
