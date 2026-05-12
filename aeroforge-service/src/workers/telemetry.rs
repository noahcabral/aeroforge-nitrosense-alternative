mod acer_hid_status;
mod cache;
mod cpu;
mod firmware;
mod gpu;
mod hardware_identity;
mod models;
mod power;
mod system;

use models::{LowLevelSnapshot, TelemetrySnapshot};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::{run_periodic_worker, WorkerEventSender, WorkerRegistration},
};
use std::sync::{Mutex, OnceLock};

static NVIDIA_TELEMETRY_STATE_LOG: OnceLock<Mutex<Option<bool>>> = OnceLock::new();

pub fn registration() -> WorkerRegistration {
    WorkerRegistration::new("telemetry-worker", run)
}

fn run(
    paths: ServicePaths,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_periodic_worker(
        "telemetry-worker",
        std::time::Duration::from_millis(333),
        paths,
        stop_flag,
        event_tx,
        tick,
    )
}

fn tick(paths: &ServicePaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let heartbeat = system::next_heartbeat();
    let power = power::read_power_snapshot().unwrap_or_default();
    let cpu_usage = cpu::read_cpu_usage_percent().unwrap_or(0);
    let cpu_clock_mhz = cpu::read_cpu_clock_mhz(paths);
    let firmware = firmware::read_firmware_sensors(paths);
    let acer_hid_status = acer_hid_status::read_status_snapshot();
    let gpu = read_gpu_snapshot(paths);
    let hardware_identity = hardware_identity::read_hardware_identity(paths);
    let low_level = read_low_level_snapshot(paths).unwrap_or_default();
    let cpu_thermal = cpu::build_cpu_thermal_snapshot(&low_level, &firmware);

    let snapshot = TelemetrySnapshot {
        cpu_temp_c: cpu_thermal
            .average_temp_c
            .or(acer_hid_status.cpu_temp_c)
            .unwrap_or(0),
        cpu_temp_average_c: cpu_thermal.average_temp_c.or(acer_hid_status.cpu_temp_c),
        cpu_temp_lowest_core_c: cpu_thermal.lowest_core_temp_c,
        cpu_temp_highest_core_c: cpu_thermal.highest_core_temp_c,
        gpu_temp_c: gpu.temp_c.or(firmware.gpu_temp_c).unwrap_or(0),
        system_temp_c: system::select_system_temp_c(
            firmware
                .system_temp_c
                .or(firmware.thermal_zone_temp_c)
                .or(acer_hid_status.system_temp_c),
        )
        .unwrap_or(0),
        cpu_usage_percent: cpu_usage,
        gpu_usage_percent: gpu.usage_percent.unwrap_or(0),
        gpu_memory_usage_percent: gpu.memory_usage_percent,
        gpu_power_draw_w: gpu.power_draw_w,
        gpu_power_limit_w: gpu.power_limit_w,
        gpu_power_default_limit_w: gpu.power_default_limit_w,
        gpu_power_min_limit_w: gpu.power_min_limit_w,
        gpu_power_max_limit_w: gpu.power_max_limit_w,
        cpu_package_power_w: low_level.package_power_w,
        cpu_pl1_w: low_level.package_pl1_w,
        cpu_pl1_enabled: low_level.package_pl1_enabled,
        cpu_pl2_w: low_level.package_pl2_w,
        cpu_pl2_enabled: low_level.package_pl2_enabled,
        cpu_power_limit_locked: low_level.package_power_limit_locked,
        cpu_name: hardware_identity.cpu_name.clone(),
        cpu_brand: hardware_identity.cpu_brand.clone(),
        gpu_name: hardware_identity.gpu_name.clone(),
        gpu_brand: hardware_identity.gpu_brand.clone(),
        system_vendor: hardware_identity.system_vendor.clone(),
        system_model: hardware_identity.system_model.clone(),
        cpu_clock_mhz,
        gpu_clock_mhz: gpu.clock_mhz.unwrap_or(0),
        cpu_fan_rpm: acer_hid_status
            .cpu_fan_rpm
            .or(firmware.cpu_fan_rpm)
            .unwrap_or(0),
        gpu_fan_rpm: acer_hid_status
            .gpu_fan_rpm
            .or(firmware.gpu_fan_rpm)
            .unwrap_or(0),
        battery_percent: power.battery_percent,
        battery_life_remaining_sec: power.battery_life_remaining_sec,
        ac_plugged_in: power.ac_plugged_in,
        heartbeat,
    };

    std::fs::write(
        paths.worker_snapshot("telemetry"),
        serde_json::to_string_pretty(&snapshot)?,
    )?;

    Ok(())
}

fn read_gpu_snapshot(paths: &ServicePaths) -> models::GpuSnapshot {
    if !nvidia_telemetry_enabled(paths) {
        return models::GpuSnapshot::default();
    }

    gpu::read_gpu_snapshot(paths)
}

fn nvidia_telemetry_enabled(paths: &ServicePaths) -> bool {
    let enabled = std::fs::read_to_string(paths.worker_snapshot("control"))
        .ok()
        .and_then(|raw| {
            serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}')).ok()
        })
        .and_then(|snapshot| {
            snapshot
                .get("nvidiaTelemetryEnabled")
                .and_then(|value| value.as_bool())
        })
        .unwrap_or(true);

    log_nvidia_telemetry_state(paths, enabled);
    enabled
}

fn log_nvidia_telemetry_state(paths: &ServicePaths, enabled: bool) {
    let cache = NVIDIA_TELEMETRY_STATE_LOG.get_or_init(|| Mutex::new(None));
    let Ok(mut last_enabled) = cache.lock() else {
        return;
    };

    if last_enabled.as_ref() == Some(&enabled) {
        return;
    }

    *last_enabled = Some(enabled);
    let detail = if enabled {
        "NVIDIA telemetry polling is enabled."
    } else {
        "NVIDIA telemetry polling is disabled; skipping NVML and nvidia-smi reads."
    };
    let _ = write_log_line(&paths.component_log("telemetry-nvidia-gpu"), "INFO", detail);
}

fn read_low_level_snapshot(
    paths: &ServicePaths,
) -> Result<LowLevelSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let raw = std::fs::read_to_string(paths.worker_snapshot("lowlevel"))?;
    Ok(serde_json::from_str::<LowLevelSnapshot>(&raw)?)
}
