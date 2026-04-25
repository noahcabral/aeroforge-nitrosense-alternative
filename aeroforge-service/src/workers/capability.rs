mod models;
mod probes;

use models::{feature, CapabilitySnapshot, FeatureSupport};

use crate::{
    paths::ServicePaths,
    workers::{run_periodic_worker, WorkerEventSender, WorkerRegistration},
};

const WORKER_NAME: &str = "capability-worker";
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

pub fn registration() -> WorkerRegistration {
    WorkerRegistration::new(WORKER_NAME, run)
}

fn run(
    paths: ServicePaths,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_periodic_worker(
        WORKER_NAME,
        SAMPLE_INTERVAL,
        paths,
        stop_flag,
        event_tx,
        tick,
    )
}

fn tick(paths: &ServicePaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let nvml_present = probes::nvml_present();
    let snapshot = CapabilitySnapshot {
        power_profiles: feature(true, true),
        fan_profiles: feature(true, true),
        fan_curves: feature(true, true),
        smart_charging: feature(false, true),
        usb_power: feature(false, true),
        blue_light_filter: feature(false, false),
        gpu_tuning: FeatureSupport {
            available: nvml_present,
            writable: false,
            requires_elevation: true,
        },
        boot_logo: feature(false, true),
        notes: vec![
            "Service capabilities are now delivered over the AeroForge named pipe.".into(),
            "Power-profile application now writes processor min and max state through Windows powercfg on the active scheme.".into(),
            "Fan profile writes use direct ROOT\\WMI AcerGamingFunction ACPI calls; RPM movement is verified separately through telemetry.".into(),
        ],
    };

    std::fs::write(
        paths.worker_snapshot("capabilities"),
        serde_json::to_string_pretty(&snapshot)?,
    )?;

    Ok(())
}
