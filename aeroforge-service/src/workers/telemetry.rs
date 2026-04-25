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
    paths::ServicePaths,
    workers::{run_periodic_worker, WorkerEventSender, WorkerRegistration},
};

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
    let gpu = gpu::read_gpu_snapshot().unwrap_or_default();
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
        gpu_temp_c: gpu.temp_c.unwrap_or(0),
        system_temp_c: system::select_system_temp_c(
            firmware
                .thermal_zone_temp_c
                .or(acer_hid_status.system_temp_c),
        )
        .unwrap_or(0),
        cpu_usage_percent: cpu_usage,
        gpu_usage_percent: gpu.usage_percent.unwrap_or(0),
        gpu_memory_usage_percent: gpu.memory_usage_percent,
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

fn read_low_level_snapshot(
    paths: &ServicePaths,
) -> Result<LowLevelSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let raw = std::fs::read_to_string(paths.worker_snapshot("lowlevel"))?;
    Ok(serde_json::from_str::<LowLevelSnapshot>(&raw)?)
}
