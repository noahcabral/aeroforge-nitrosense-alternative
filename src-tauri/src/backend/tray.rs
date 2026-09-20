use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, Wry,
};

use super::{
    commands::off_main_fallible,
    models::{ControlSnapshot, FanProfileId, PowerProfileId, ProcessorStateSettings},
    service_pipe, smart_charge,
    state::BackendState,
};

const TRAY_ID: &str = "aeroforge-main-tray";
const ID_OPEN: &str = "tray-open";
const ID_EXIT: &str = "tray-exit";
const ID_RESTORE_BALANCED_AUTO: &str = "tray-restore-balanced-auto";
const ID_FAN_AUTO: &str = "tray-fan-auto";
const ID_FAN_COOL: &str = "tray-fan-cool";
const ID_FAN_MAX: &str = "tray-fan-max";
const ID_POWER_QUIET: &str = "tray-power-quiet";
const ID_POWER_BALANCED: &str = "tray-power-balanced";
const ID_POWER_PERFORMANCE: &str = "tray-power-performance";
const ID_POWER_TURBO: &str = "tray-power-turbo";
const ID_CHARGE_OPTIMIZED: &str = "tray-charge-optimized";
const ID_CHARGE_FULL: &str = "tray-charge-full";

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrayControlEvent {
    action: String,
    detail: String,
    controls: ControlSnapshot,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrayControlErrorEvent {
    action: String,
    detail: String,
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let controls = app.state::<BackendState>().controls();
    let menu = build_tray_menu(app, &controls)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("AeroForge Control")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();
            handle_menu_event(app.clone(), id);
        })
        .on_tray_icon_event(|tray, event| {
            if should_open_from_tray_click(&event) {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    refresh(app.handle(), &controls);
    Ok(())
}

pub fn exit_requested() -> bool {
    EXIT_REQUESTED.load(Ordering::Relaxed)
}

/// Rebuild tray checkmarks and tooltip from the latest saved control snapshot.
pub fn refresh(app: &AppHandle, controls: &ControlSnapshot) {
    if let Err(error) = sync_tray_menu(app, controls) {
        eprintln!("AeroForge tray menu refresh failed: {error}");
    }
    if let Err(error) = update_tray_tooltip(app, controls) {
        eprintln!("AeroForge tray tooltip refresh failed: {error}");
    }
}

fn update_tray_tooltip(app: &AppHandle, controls: &ControlSnapshot) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let power = match controls.active_power_profile {
        PowerProfileId::BatteryGuard => "Quiet",
        PowerProfileId::Balanced => "Balanced",
        PowerProfileId::Performance => "Performance",
        PowerProfileId::Turbo => "Turbo",
        PowerProfileId::Custom => "Custom",
    };
    let fan = match controls.active_fan_profile {
        FanProfileId::Auto => "Auto",
        FanProfileId::Cool => "Cool",
        FanProfileId::Max => "Max",
        FanProfileId::Custom => "Custom",
    };
    let charge = if controls.personal_settings.smart_charging_enabled {
        "80% charge"
    } else {
        "Full charge"
    };

    tray.set_tooltip(Some(format!("AeroForge · {power} · Fans {fan} · {charge}")))?;
    Ok(())
}

fn build_tray_menu<M: Manager<Wry>>(
    manager: &M,
    controls: &ControlSnapshot,
) -> tauri::Result<Menu<Wry>> {
    let open = MenuItem::with_id(manager, ID_OPEN, "Open AeroForge", true, None::<&str>)?;
    let restore = MenuItem::with_id(
        manager,
        ID_RESTORE_BALANCED_AUTO,
        "Restore Balanced + Auto",
        true,
        None::<&str>,
    )?;
    let exit = MenuItem::with_id(manager, ID_EXIT, "Exit", true, None::<&str>)?;

    let fan_auto = CheckMenuItem::with_id(
        manager,
        ID_FAN_AUTO,
        "Auto",
        true,
        matches!(controls.active_fan_profile, FanProfileId::Auto),
        None::<&str>,
    )?;
    let fan_cool = CheckMenuItem::with_id(
        manager,
        ID_FAN_COOL,
        "Cool",
        true,
        matches!(controls.active_fan_profile, FanProfileId::Cool),
        None::<&str>,
    )?;
    let fan_max = CheckMenuItem::with_id(
        manager,
        ID_FAN_MAX,
        "Max",
        true,
        matches!(controls.active_fan_profile, FanProfileId::Max),
        None::<&str>,
    )?;
    let fan_menu = Submenu::with_items(manager, "Fan", true, &[&fan_auto, &fan_cool, &fan_max])?;

    let power_quiet = CheckMenuItem::with_id(
        manager,
        ID_POWER_QUIET,
        "Quiet",
        true,
        matches!(controls.active_power_profile, PowerProfileId::BatteryGuard),
        None::<&str>,
    )?;
    let power_balanced = CheckMenuItem::with_id(
        manager,
        ID_POWER_BALANCED,
        "Balanced",
        true,
        matches!(controls.active_power_profile, PowerProfileId::Balanced),
        None::<&str>,
    )?;
    let power_performance = CheckMenuItem::with_id(
        manager,
        ID_POWER_PERFORMANCE,
        "Performance",
        true,
        matches!(controls.active_power_profile, PowerProfileId::Performance),
        None::<&str>,
    )?;
    let power_turbo = CheckMenuItem::with_id(
        manager,
        ID_POWER_TURBO,
        "Turbo",
        true,
        matches!(controls.active_power_profile, PowerProfileId::Turbo),
        None::<&str>,
    )?;
    let power_menu = Submenu::with_items(
        manager,
        "Power",
        true,
        &[
            &power_quiet,
            &power_balanced,
            &power_performance,
            &power_turbo,
        ],
    )?;

    let charge_optimized = CheckMenuItem::with_id(
        manager,
        ID_CHARGE_OPTIMIZED,
        "Optimized",
        true,
        controls.personal_settings.smart_charging_enabled,
        None::<&str>,
    )?;
    let charge_full = CheckMenuItem::with_id(
        manager,
        ID_CHARGE_FULL,
        "Full",
        true,
        !controls.personal_settings.smart_charging_enabled,
        None::<&str>,
    )?;
    let charge_menu = Submenu::with_items(
        manager,
        "Smart Charging",
        true,
        &[&charge_optimized, &charge_full],
    )?;

    let sep_one = PredefinedMenuItem::separator(manager)?;
    let sep_two = PredefinedMenuItem::separator(manager)?;
    let sep_three = PredefinedMenuItem::separator(manager)?;

    Menu::with_items(
        manager,
        &[
            &open,
            &sep_one,
            &fan_menu,
            &power_menu,
            &charge_menu,
            &sep_two,
            &restore,
            &sep_three,
            &exit,
        ],
    )
}

fn handle_menu_event(app: AppHandle, id: String) {
    match id.as_str() {
        ID_OPEN => show_main_window(&app),
        ID_EXIT => {
            EXIT_REQUESTED.store(true, Ordering::Relaxed);
            app.exit(0);
        }
        _ => {
            tauri::async_runtime::spawn(async move {
                let action = menu_action_label(&id).to_string();
                match apply_menu_action(&app, &id).await {
                    Ok((controls, detail)) => {
                        refresh(&app, &controls);
                        let _ = app.emit(
                            "tray-control-updated",
                            TrayControlEvent {
                                action,
                                detail,
                                controls,
                            },
                        );
                    }
                    Err(detail) => {
                        show_main_window(&app);
                        show_tray_error(&app, &action, detail);
                    }
                }
            });
        }
    }
}

async fn apply_menu_action(app: &AppHandle, id: &str) -> Result<(ControlSnapshot, String), String> {
    match id {
        ID_FAN_AUTO => apply_tray_fan_profile(app, FanProfileId::Auto).await,
        ID_FAN_COOL => apply_tray_fan_profile(app, FanProfileId::Cool).await,
        ID_FAN_MAX => apply_tray_fan_profile(app, FanProfileId::Max).await,
        ID_POWER_QUIET => apply_tray_power_profile(app, PowerProfileId::BatteryGuard).await,
        ID_POWER_BALANCED => apply_tray_power_profile(app, PowerProfileId::Balanced).await,
        ID_POWER_PERFORMANCE => apply_tray_power_profile(app, PowerProfileId::Performance).await,
        ID_POWER_TURBO => apply_tray_power_profile(app, PowerProfileId::Turbo).await,
        ID_CHARGE_OPTIMIZED => apply_tray_smart_charging(app, true).await,
        ID_CHARGE_FULL => apply_tray_smart_charging(app, false).await,
        ID_RESTORE_BALANCED_AUTO => restore_balanced_auto(app).await,
        _ => Err(format!("Unknown tray action '{id}'.")),
    }
}

async fn apply_tray_fan_profile(
    app: &AppHandle,
    profile_id: FanProfileId,
) -> Result<(ControlSnapshot, String), String> {
    let applied = off_main_fallible("Fan profile apply", move || {
        service_pipe::apply_fan_profile(profile_id)
    })
    .await?;
    let mut controls = app.state::<BackendState>().controls();
    controls.active_fan_profile = applied.profile_id;
    let controls = app
        .state::<BackendState>()
        .save_controls(controls)
        .map_err(|error| error.to_string())?;
    let detail = if applied.detail.trim().is_empty() {
        format!(
            "Applied {:?} fan mode from the tray.",
            controls.active_fan_profile
        )
    } else {
        applied.detail
    };
    Ok((controls, detail))
}

async fn apply_tray_power_profile(
    app: &AppHandle,
    profile_id: PowerProfileId,
) -> Result<(ControlSnapshot, String), String> {
    let processor_state_control_enabled = app
        .state::<BackendState>()
        .controls()
        .personal_settings
        .processor_state_control_enabled;
    let processor_state = default_processor_state_for_profile(&profile_id);
    let applied = off_main_fallible("Power profile apply", move || {
        service_pipe::apply_power_profile(
            profile_id,
            processor_state,
            None,
            processor_state_control_enabled,
        )
    })
    .await?;

    let mut controls = app.state::<BackendState>().controls();
    controls.active_power_profile = applied.profile_id;
    controls.personal_settings.processor_state_control_enabled =
        applied.processor_state_control_enabled;
    let controls = app
        .state::<BackendState>()
        .save_controls(controls)
        .map_err(|error| error.to_string())?;
    Ok((controls, applied.detail))
}

async fn apply_tray_smart_charging(
    app: &AppHandle,
    enabled: bool,
) -> Result<(ControlSnapshot, String), String> {
    let applied = smart_charge::apply_with_desktop_fallback(enabled).await?;

    let mut controls = app.state::<BackendState>().controls();
    controls.personal_settings.smart_charging_enabled = applied.enabled;
    let controls = app
        .state::<BackendState>()
        .save_controls(controls)
        .map_err(|error| error.to_string())?;
    Ok((controls, applied.detail))
}

async fn restore_balanced_auto(app: &AppHandle) -> Result<(ControlSnapshot, String), String> {
    let (_, power_detail) = apply_tray_power_profile(app, PowerProfileId::Balanced).await?;
    let (controls, fan_detail) = apply_tray_fan_profile(app, FanProfileId::Auto).await?;
    Ok((
        controls,
        format!("Restored Balanced power and Auto fans. {power_detail} {fan_detail}"),
    ))
}

fn sync_tray_menu(app: &AppHandle, controls: &ControlSnapshot) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_tray_menu(app, controls)?))?;
    }
    Ok(())
}

fn show_tray_error(app: &AppHandle, action: &str, detail: String) {
    let _ = app.emit(
        "tray-control-error",
        TrayControlErrorEvent {
            action: action.to_string(),
            detail,
        },
    );
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn should_open_from_tray_click(event: &TrayIconEvent) -> bool {
    match event {
        TrayIconEvent::Click {
            button,
            button_state,
            ..
        } => *button == MouseButton::Left && *button_state == MouseButtonState::Up,
        TrayIconEvent::DoubleClick { button, .. } => *button == MouseButton::Left,
        _ => false,
    }
}

fn default_processor_state_for_profile(profile_id: &PowerProfileId) -> ProcessorStateSettings {
    match profile_id {
        PowerProfileId::BatteryGuard => ProcessorStateSettings {
            min_percent: 5,
            max_percent: 45,
        },
        PowerProfileId::Balanced => ProcessorStateSettings {
            min_percent: 35,
            max_percent: 88,
        },
        PowerProfileId::Performance | PowerProfileId::Turbo => ProcessorStateSettings {
            min_percent: 100,
            max_percent: 100,
        },
        PowerProfileId::Custom => ProcessorStateSettings {
            min_percent: 35,
            max_percent: 88,
        },
    }
}

fn menu_action_label(id: &str) -> &'static str {
    match id {
        ID_FAN_AUTO => "Fan: Auto",
        ID_FAN_COOL => "Fan: Cool",
        ID_FAN_MAX => "Fan: Max",
        ID_POWER_QUIET => "Power: Quiet",
        ID_POWER_BALANCED => "Power: Balanced",
        ID_POWER_PERFORMANCE => "Power: Performance",
        ID_POWER_TURBO => "Power: Turbo",
        ID_CHARGE_OPTIMIZED => "Smart Charging: Optimized",
        ID_CHARGE_FULL => "Smart Charging: Full",
        ID_RESTORE_BALANCED_AUTO => "Restore Balanced + Auto",
        _ => "Tray Action",
    }
}

#[cfg(test)]
mod tests {
    use super::default_processor_state_for_profile;
    use crate::backend::models::PowerProfileId;

    #[test]
    fn tray_power_actions_use_same_default_processor_targets_as_ui() {
        let quiet = default_processor_state_for_profile(&PowerProfileId::BatteryGuard);
        assert_eq!(quiet.min_percent, 5);
        assert_eq!(quiet.max_percent, 45);

        let balanced = default_processor_state_for_profile(&PowerProfileId::Balanced);
        assert_eq!(balanced.min_percent, 35);
        assert_eq!(balanced.max_percent, 88);

        let performance = default_processor_state_for_profile(&PowerProfileId::Performance);
        assert_eq!(performance.min_percent, 100);
        assert_eq!(performance.max_percent, 100);
    }
}
