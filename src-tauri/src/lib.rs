pub mod backend;

use std::env;

use backend::{
    app_rules::{self, AppRuleStore},
    blue_light, commands, nitro_guard, nitro_key, service_pipe, single_instance, smart_charge,
    state::BackendState, tray,
};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let prewarm_mode = env::args()
        .skip(1)
        .any(|arg| arg == "--prewarm" || arg == "--background");

    if single_instance::activate_existing_instance(!prewarm_mode) {
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_hardware_readiness_snapshot,
            commands::repair_hardware_prerequisites,
            commands::request_service_heal,
            commands::get_capability_snapshot,
            commands::get_control_snapshot,
            commands::get_live_control_snapshot,
            commands::get_backend_poll_snapshot,
            commands::get_backend_bootstrap,
            commands::check_for_updates,
            commands::stage_update_download,
            commands::install_staged_update,
            commands::show_update_notification,
            commands::show_thermal_warning_notification,
            commands::append_performance_log,
            commands::apply_blue_light_filter,
            commands::apply_smart_charging,
            commands::apply_auto_refresh_rate,
            commands::set_nvidia_telemetry_enabled,
            commands::save_control_snapshot,
            commands::apply_power_profile,
            commands::apply_gpu_tuning,
            commands::apply_fan_profile,
            commands::apply_custom_fan_curves,
            commands::start_fan_speed_calibration,
            commands::cancel_fan_speed_calibration,
            commands::apply_boot_logo,
            commands::get_app_rules,
            commands::save_app_rules
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if tray::exit_requested() {
                    return;
                }

                api.prevent_close();
                let window = window.clone();
                let _ = window.emit("tray-window-will-hide", ());
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    let _ = window.hide();
                });
            }
        })
        .setup(move |app| {
            let config_root = app.path().app_config_dir()?;
            let backend_state = BackendState::load(config_root.clone())
                .map_err(|error| -> Box<dyn std::error::Error> { error })?;
            let saved_blue_light_state = backend_state
                .controls()
                .personal_settings
                .blue_light_filter_enabled;
            let saved_smart_charge_state = backend_state
                .controls()
                .personal_settings
                .smart_charging_enabled;
            let saved_nvidia_telemetry_state = backend_state
                .controls()
                .personal_settings
                .nvidia_telemetry_enabled;
            app.manage(AppRuleStore::load(&config_root));
            app.manage(backend_state);
            nitro_guard::start();
            app_rules::start(app.handle().clone());
            nitro_key::start(app.handle().clone());

            if let Err(error) = tray::setup(app) {
                eprintln!("AeroForge tray setup failed: {error}");
            }

            if cfg!(debug_assertions) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title("AeroForge Control [DEV]");
                }

                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // A prewarmed instance becomes the user's real instance once activated, so it
            // still runs the startup reconcile (gamma ramp, smart charge, telemetry); it only
            // skips showing the window.
            if let Some(window) = app.get_webview_window("main") {
                if prewarm_mode {
                    let _ = window.hide();
                } else {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            tauri::async_runtime::spawn_blocking(move || {
                run_startup_reconcile(
                    saved_blue_light_state,
                    saved_smart_charge_state,
                    saved_nvidia_telemetry_state,
                );
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn run_startup_reconcile(
    saved_blue_light_state: bool,
    saved_smart_charge_state: bool,
    saved_nvidia_telemetry_state: bool,
) {
    if let Err(error) = service_pipe::ensure_service_running() {
        eprintln!("AeroForge service startup check failed: {error}");
    }
    if let Err(error) = service_pipe::restore_startup_state() {
        eprintln!("AeroForge manual-launch restore failed during startup: {error}");
    }
    if let Err(error) = blue_light::apply_blue_light_filter(saved_blue_light_state) {
        eprintln!("AeroForge blue-light sync failed during startup: {error}");
    }
    if let Err(error) = tauri::async_runtime::block_on(smart_charge::apply_with_desktop_fallback(
        saved_smart_charge_state,
    )) {
        eprintln!("AeroForge smart-charge sync failed during startup: {error}");
    }
    if let Err(error) = service_pipe::apply_telemetry_settings(saved_nvidia_telemetry_state) {
        eprintln!("AeroForge NVIDIA telemetry sync failed during startup: {error}");
    }
}
