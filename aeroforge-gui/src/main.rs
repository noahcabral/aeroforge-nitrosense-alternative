slint::include_modules!();

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use aeroforge_common::{
    ServiceClient, TelemetrySnapshot, ControlSnapshot, ServiceStatus,
    PowerProfileId, ProcessorStateSettings, FanProfileId, GpuTuningState,
};

mod worker;
mod system;

use worker::TelemetryWorker;
use system::{create_single_instance_mutex, setup_nitro_key_listener};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Single-instance guard
    let _instance_guard = create_single_instance_mutex()?;

    // Initialize Slint UI
    let ui = AppWindow::new()?;
    let ui_weak = ui.as_weak();

    // Shared state for telemetry worker
    let telemetry_state = Arc::new(Mutex::new(TelemetrySnapshot {
        cpu_temp_c: 0,
        cpu_temp_average_c: None,
        cpu_temp_lowest_core_c: None,
        cpu_temp_highest_core_c: None,
        gpu_temp_c: 0,
        system_temp_c: 0,
        cpu_usage_percent: 0,
        gpu_usage_percent: 0,
        gpu_memory_usage_percent: None,
        gpu_power_draw_w: None,
        gpu_power_limit_w: None,
        gpu_power_default_limit_w: None,
        gpu_power_min_limit_w: None,
        gpu_power_max_limit_w: None,
        cpu_package_power_w: None,
        cpu_pl1_w: None,
        cpu_pl1_enabled: None,
        cpu_pl2_w: None,
        cpu_pl2_enabled: None,
        cpu_power_limit_locked: None,
        cpu_name: None,
        cpu_brand: None,
        gpu_name: None,
        gpu_brand: None,
        system_vendor: None,
        system_model: None,
        cpu_clock_mhz: 0,
        gpu_clock_mhz: 0,
        cpu_fan_rpm: 0,
        gpu_fan_rpm: 0,
        battery_percent: 0,
        battery_life_remaining_sec: None,
        ac_plugged_in: false,
        heartbeat: 0,
    }));

    let service_state = Arc::new(Mutex::new(ServiceStatus {
        connected: false,
        pipe_name: String::new(),
        service_name: String::new(),
        version: None,
        state_dir: None,
        supervisor_file: None,
        worker_count: 0,
        updated_at_unix: None,
        workers: Vec::new(),
        detail: "Initializing...".to_string(),
    }));

    let control_state = Arc::new(Mutex::new(ControlSnapshot {
        service: String::new(),
        power_apply_supported: true,
        gpu_tuning_apply_supported: true,
        fan_apply_supported: true,
        fan_curve_apply_supported: true,
        active_power_profile: Some(PowerProfileId::Turbo),
        processor_state: Some(ProcessorStateSettings {
            min_percent: 100,
            max_percent: 100,
        }),
        custom_base_profile: None,
        processor_state_control_enabled: true,
        nvidia_telemetry_enabled: true,
        processor_state_readback: None,
        processor_state_drift_detected: false,
        last_applied_at_unix: None,
        last_apply_detail: "Waiting for first apply.".to_string(),
        last_error: None,
        active_gpu_tuning: None,
        last_gpu_tuning_applied_at_unix: None,
        last_gpu_tuning_detail: "Waiting for first GPU tuning apply.".to_string(),
        last_gpu_tuning_error: None,
        active_fan_profile: Some(FanProfileId::Auto),
        active_fan_curves: None,
        current_cpu_fan_speed_percent: None,
        current_gpu_fan_speed_percent: None,
        last_fan_applied_at_unix: None,
        last_fan_apply_detail: "Waiting for first fan apply.".to_string(),
        last_fan_error: None,
        last_fan_readback: None,
        quiet_auto_fan_calibration: Default::default(),
        quiet_auto_thermal_warning: None,
        fan_speed_calibration: Default::default(),
        boot_logo_apply_supported: true,
        last_boot_logo_applied_at_unix: None,
        last_boot_logo_apply_detail: "Ready.".to_string(),
        last_boot_logo_error: None,
        last_boot_logo_readback: None,
    }));

    // Start telemetry worker thread
    let worker_running = Arc::new(AtomicBool::new(true));
    let worker = TelemetryWorker::new(
        telemetry_state.clone(),
        service_state.clone(),
        control_state.clone(),
        worker_running.clone(),
    );

    let worker_handle = thread::spawn(move || {
        worker.run();
    });

    // Setup UI update timer (16ms = 60 FPS)
    let ui_weak_timer = ui_weak.clone();
    let telemetry_clone = telemetry_state.clone();
    let service_clone = service_state.clone();
    let control_clone = control_state.clone();

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            if let Some(ui) = ui_weak_timer.upgrade() {
                // Update telemetry display
                if let Ok(telemetry) = telemetry_clone.lock() {
                    ui.set_telemetry(TelemetryData {
                        cpu_temp: telemetry.cpu_temp_c as i32,
                        gpu_temp: telemetry.gpu_temp_c as i32,
                        cpu_power: telemetry.cpu_package_power_w.unwrap_or(0.0),
                        gpu_power: telemetry.gpu_power_draw_w.unwrap_or(0.0),
                        cpu_fan_rpm: telemetry.cpu_fan_rpm as i32,
                        gpu_fan_rpm: telemetry.gpu_fan_rpm as i32,
                        cpu_clock: telemetry.cpu_clock_mhz as i32,
                        gpu_clock: telemetry.gpu_clock_mhz as i32,
                        battery_percent: telemetry.battery_percent as i32,
                        ac_plugged: telemetry.ac_plugged_in,
                    });
                }

                // Update service connection state
                if let Ok(service) = service_clone.lock() {
                    ui.set_service_state(ServiceState {
                        connected: service.connected,
                        detail: service.detail.clone().into(),
                    });
                }

                // Update active profiles
                if let Ok(control) = control_clone.lock() {
                    if let Some(ref profile) = control.active_power_profile {
                        ui.set_active_power_profile(profile_id_to_string(profile).into());
                    }
                    if let Some(ref profile) = control.active_fan_profile {
                        ui.set_active_fan_profile(fan_profile_id_to_string(profile).into());
                    }
                }
            }
        },
    );

    // Setup power profile callback
    let ui_weak_power = ui_weak.clone();
    ui.on_apply_power_profile(move |profile_str| {
        let profile_id = string_to_profile_id(&profile_str);
        let processor_state = default_processor_state_for_profile(&profile_id);

        thread::spawn(move || {
            match ServiceClient::apply_power_profile(
                profile_id,
                processor_state,
                None,
                true,
            ) {
                Ok(_) => println!("Power profile applied successfully"),
                Err(e) => eprintln!("Failed to apply power profile: {}", e),
            }
        });
    });

    // Setup fan profile callback
    let ui_weak_fan = ui_weak.clone();
    ui.on_apply_fan_profile(move |profile_str| {
        let profile_id = string_to_fan_profile_id(&profile_str);

        thread::spawn(move || {
            match ServiceClient::apply_fan_profile(profile_id) {
                Ok(_) => println!("Fan profile applied successfully"),
                Err(e) => eprintln!("Failed to apply fan profile: {}", e),
            }
        });
    });

    // Setup Nitro key listener (Win+N to show window)
    setup_nitro_key_listener(ui_weak.clone());

    // Run the UI event loop
    ui.run()?;

    // Cleanup: stop worker thread
    worker_running.store(false, Ordering::Relaxed);
    let _ = worker_handle.join();

    Ok(())
}

fn profile_id_to_string(id: &PowerProfileId) -> String {
    match id {
        PowerProfileId::BatteryGuard => "battery-guard".to_string(),
        PowerProfileId::Balanced => "balanced".to_string(),
        PowerProfileId::Performance => "performance".to_string(),
        PowerProfileId::Turbo => "turbo".to_string(),
        PowerProfileId::Custom => "custom".to_string(),
    }
}

fn string_to_profile_id(s: &str) -> PowerProfileId {
    match s {
        "battery-guard" => PowerProfileId::BatteryGuard,
        "balanced" => PowerProfileId::Balanced,
        "performance" => PowerProfileId::Performance,
        "turbo" => PowerProfileId::Turbo,
        "custom" => PowerProfileId::Custom,
        _ => PowerProfileId::Turbo,
    }
}

fn fan_profile_id_to_string(id: &FanProfileId) -> String {
    match id {
        FanProfileId::Auto => "auto".to_string(),
        FanProfileId::Max => "max".to_string(),
        FanProfileId::Custom => "custom".to_string(),
    }
}

fn string_to_fan_profile_id(s: &str) -> FanProfileId {
    match s {
        "auto" => FanProfileId::Auto,
        "max" => FanProfileId::Max,
        "custom" => FanProfileId::Custom,
        _ => FanProfileId::Auto,
    }
}

fn default_processor_state_for_profile(profile: &PowerProfileId) -> ProcessorStateSettings {
    match profile {
        PowerProfileId::BatteryGuard => ProcessorStateSettings {
            min_percent: 5,
            max_percent: 80,
        },
        PowerProfileId::Balanced => ProcessorStateSettings {
            min_percent: 5,
            max_percent: 100,
        },
        PowerProfileId::Performance => ProcessorStateSettings {
            min_percent: 20,
            max_percent: 100,
        },
        PowerProfileId::Turbo => ProcessorStateSettings {
            min_percent: 100,
            max_percent: 100,
        },
        PowerProfileId::Custom => ProcessorStateSettings {
            min_percent: 50,
            max_percent: 100,
        },
    }
}
