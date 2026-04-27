mod backend;

use backend::{
    blue_light, commands, nitro_guard, nitro_key, single_instance, smart_charge,
    state::BackendState,
};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if single_instance::activate_existing_instance() {
        return;
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::runtime_shell,
            commands::get_backend_contract,
            commands::get_service_status,
            commands::get_capability_snapshot,
            commands::get_control_snapshot,
            commands::get_live_control_snapshot,
            commands::get_telemetry_snapshot,
            commands::get_backend_bootstrap,
            commands::get_persistence_status,
            commands::get_update_status,
            commands::check_for_updates,
            commands::stage_update_download,
            commands::install_staged_update,
            commands::apply_blue_light_filter,
            commands::apply_smart_charging,
            commands::save_control_snapshot,
            commands::reset_control_snapshot,
            commands::apply_power_profile,
            commands::apply_gpu_tuning,
            commands::apply_fan_profile,
            commands::apply_custom_fan_curves,
            commands::apply_boot_logo
        ])
        .setup(move |app| {
            let config_root = app.path().app_config_dir()?;
            let backend_state = BackendState::load(config_root)
                .map_err(|error| -> Box<dyn std::error::Error> { error })?;
            let saved_blue_light_state = backend_state
                .controls()
                .personal_settings
                .blue_light_filter_enabled;
            let saved_smart_charge_state = backend_state
                .controls()
                .personal_settings
                .smart_charging_enabled;
            app.manage(backend_state);
            nitro_guard::start();
            nitro_key::start(app.handle().clone());

            if let Err(error) = blue_light::sync_saved_state(saved_blue_light_state) {
                eprintln!("AeroForge blue-light sync failed during startup: {error}");
            }
            if let Err(error) = tauri::async_runtime::block_on(smart_charge::sync_saved_state(
                saved_smart_charge_state,
            )) {
                eprintln!("AeroForge smart-charge sync failed during startup: {error}");
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

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
