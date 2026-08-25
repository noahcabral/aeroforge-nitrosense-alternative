mod models;
mod probes;

use models::{feature, feature_with, CapabilitySnapshot, FeatureSupport};

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
    let acer_wmi_present = probes::acer_gaming_wmi_present();
    let acer_fan_telemetry_present = probes::acer_fan_telemetry_present();
    let battery_control = probes::battery_control_probe();
    let nvml_present = probes::nvml_present();
    let snapshot = CapabilitySnapshot {
        power_profiles: feature_with(acer_wmi_present, acer_wmi_present, true),
        fan_profiles: feature_with(acer_wmi_present, acer_wmi_present, true),
        fan_curves: feature_with(acer_wmi_present, acer_wmi_present, true),
        smart_charging: feature_with(
            battery_control.class_present || battery_control.instance_present,
            battery_control.health_status_readable || battery_control.instance_present,
            false,
        ),
        usb_power: feature_with(false, false, false),
        blue_light_filter: feature_with(false, false, false),
        gpu_tuning: FeatureSupport {
            available: nvml_present,
            writable: nvml_present,
            requires_elevation: true,
        },
        boot_logo: feature(true, true),
        notes: vec![
            "Service capabilities are now delivered over the AeroForge named pipe.".into(),
            if acer_wmi_present {
                "Power-profile application prefers AcerGamingFunction platform-profile misc-setting writes and then writes processor min/max state through Windows powercfg on the active scheme.".into()
            } else {
                "AcerGamingFunction WMI was not detected, so service-owned power and fan controls are unavailable on this machine or in the current session.".into()
            },
            if acer_wmi_present && acer_fan_telemetry_present {
                "Fan profile writes use direct ROOT\\WMI AcerGamingFunction ACPI calls; direct Acer fan RPM telemetry is available for verification on this machine.".into()
            } else if acer_wmi_present {
                "Fan profile writes use direct ROOT\\WMI AcerGamingFunction ACPI calls, but direct Acer fan RPM telemetry is not available in the current session, so AeroForge can only confirm command acceptance and whatever temperatures GetGamingSysInfo exposes.".into()
            } else {
                "Fan profile and curve writes stay unavailable until AcerGamingFunction WMI responds.".into()
            },
            "Firmware telemetry prefers AcerGamingFunction GetGamingSysInfo for CPU/GPU/system temperatures and fan RPMs, with generic Windows thermal-zone fallback.".into(),
            if battery_control.health_status_readable {
                "BatteryControl WMI health-status readback is available on this machine, so the desktop smart-charge path should be able to verify the 80% versus 100% battery-health mode directly.".into()
            } else if battery_control.class_present || battery_control.instance_present {
                "BatteryControl WMI is present but readback could not be confirmed in the current session. Smart-charge failures on this machine need a direct BatteryControl probe in the debug bundle.".into()
            } else {
                "BatteryControl WMI was not detected in the current session. Smart-charge support must fall back to Acer Care Center if available.".into()
            },
            "Smart charge is applied by AeroForgeService through direct BatteryControl WMI so normal user-session launches do not need elevation; blue light filter and auto-refresh still apply from the desktop backend.".into(),
            "Boot-logo apply uses a direct EFI System Partition write with strict FAT32 Windows boot-marker detection, app-owned backup, and post-write size verification.".into(),
        ],
    };

    std::fs::write(
        paths.worker_snapshot("capabilities"),
        serde_json::to_string_pretty(&snapshot)?,
    )?;

    Ok(())
}
