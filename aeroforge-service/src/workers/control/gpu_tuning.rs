use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::{
    models::{AppliedGpuTuningSnapshot, ApplyGpuTuningRequest, GpuTuningState},
    nvml,
};

pub fn apply_gpu_tuning(
    paths: &ServicePaths,
    request: ApplyGpuTuningRequest,
) -> Result<AppliedGpuTuningSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let tuning = sanitize_gpu_tuning(request.tuning);

    write_log_line(
        &paths.component_log("control-gpu-tuning"),
        "INFO",
        &format!(
            "Applying GPU tuning request: core {:+} MHz, memory {:+} MHz, voltage {:+} mV, power {}%, temp {}C.",
            tuning.core_clock_mhz,
            tuning.memory_clock_mhz,
            tuning.voltage_offset_mv,
            tuning.power_limit_percent,
            tuning.temp_limit_c
        ),
    )?;

    let nvml_report = nvml::apply_gpu_tuning(paths, &tuning);
    let mut detail_parts = Vec::new();

    let mut applied_domains = Vec::new();
    if let Some(value) = nvml_report.applied_core_clock_mhz {
        applied_domains.push(format!("core {:+} MHz", value));
    }
    if let Some(value) = nvml_report.applied_memory_clock_mhz {
        applied_domains.push(format!("memory {:+} MHz", value));
    }

    if applied_domains.is_empty() {
        detail_parts.push(format!(
            "{} did not expose any live writable GPU tuning domains for the requested tuning.",
            nvml_report.gpu_name
        ));
    } else {
        detail_parts.push(format!(
            "Applied {} on {}.",
            applied_domains.join(" and "),
            nvml_report.gpu_name
        ));
    }

    let unsupported_fields = nvml_report.unsupported_fields.clone();

    if !unsupported_fields.is_empty() {
        detail_parts.push(format!(
            "Driver does not expose live write support for {} on this path; those values remain staged only.",
            unsupported_fields.join(", ")
        ));
    }

    detail_parts.push(nvml_report.detail);

    let detail = detail_parts.join(" ");
    write_log_line(&paths.component_log("control-gpu-tuning"), "INFO", &detail)?;

    Ok(AppliedGpuTuningSnapshot {
        tuning,
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn sanitize_gpu_tuning(tuning: GpuTuningState) -> GpuTuningState {
    GpuTuningState {
        core_clock_mhz: tuning.core_clock_mhz.clamp(-250, 250),
        memory_clock_mhz: tuning.memory_clock_mhz.clamp(-1000, 1500),
        voltage_offset_mv: tuning.voltage_offset_mv.clamp(-150, 150),
        power_limit_percent: tuning.power_limit_percent.clamp(60, 125),
        temp_limit_c: tuning.temp_limit_c.clamp(65, 90),
    }
}
