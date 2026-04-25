mod backend;

use backend::{commands, nitro_guard, state::BackendState};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
            commands::set_update_token,
            commands::clear_update_token,
            commands::check_for_updates,
            commands::stage_update_download,
            commands::install_staged_update,
            commands::save_control_snapshot,
            commands::reset_control_snapshot,
            commands::apply_power_profile,
            commands::apply_gpu_tuning,
            commands::apply_fan_profile,
            commands::apply_custom_fan_curves
        ])
        .setup(|app| {
            let config_root = app.path().app_config_dir()?;
            let backend_state = BackendState::load(config_root)
                .map_err(|error| -> Box<dyn std::error::Error> { error })?;
            app.manage(backend_state);
            nitro_guard::start();

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
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
