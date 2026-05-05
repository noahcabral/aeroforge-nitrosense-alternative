use std::{
    io,
    mem::{size_of, zeroed},
    ptr::null,
    time::{SystemTime, UNIX_EPOCH},
};

use windows_sys::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsW, EnumDisplaySettingsW, CDS_TEST, DEVMODEW, DISP_CHANGE,
    DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS,
};

type DynError = Box<dyn std::error::Error + Send + Sync>;

const BATTERY_REFRESH_HZ: u32 = 60;

pub struct AutoRefreshRatePayload {
    pub enabled: bool,
    pub on_battery: bool,
    pub current_hz: u32,
    pub applied_hz: Option<u32>,
    pub restore_hz: Option<u32>,
    pub applied_at_unix: u64,
    pub detail: String,
}

pub fn sync_auto_refresh_rate(
    enabled: bool,
    on_battery: bool,
    saved_restore_hz: Option<u32>,
) -> Result<AutoRefreshRatePayload, DynError> {
    let current_hz = current_refresh_rate_hz()?;

    if !enabled {
        return sync_disabled(on_battery, current_hz, saved_restore_hz);
    }

    if on_battery {
        return sync_on_battery(current_hz, saved_restore_hz);
    }

    sync_on_ac(current_hz, saved_restore_hz)
}

fn sync_disabled(
    on_battery: bool,
    current_hz: u32,
    saved_restore_hz: Option<u32>,
) -> Result<AutoRefreshRatePayload, DynError> {
    if let Some(restore_hz) = saved_restore_hz.filter(|hz| *hz > BATTERY_REFRESH_HZ) {
        if current_hz != restore_hz {
            set_refresh_rate_hz(restore_hz)?;
            return Ok(payload(
                false,
                on_battery,
                current_hz,
                Some(restore_hz),
                None,
                format!(
                    "Auto 60 Hz on battery disabled. Restored the display refresh rate from {current_hz} Hz to {restore_hz} Hz."
                ),
            ));
        }
    }

    Ok(payload(
        false,
        on_battery,
        current_hz,
        None,
        None,
        "Auto 60 Hz on battery disabled. No refresh-rate restore was needed.".into(),
    ))
}

fn sync_on_battery(
    current_hz: u32,
    saved_restore_hz: Option<u32>,
) -> Result<AutoRefreshRatePayload, DynError> {
    if current_hz == BATTERY_REFRESH_HZ {
        return Ok(payload(
            true,
            true,
            current_hz,
            None,
            saved_restore_hz.filter(|hz| *hz > BATTERY_REFRESH_HZ),
            "Display is already running at 60 Hz on battery.".into(),
        ));
    }

    if current_hz < BATTERY_REFRESH_HZ {
        return Ok(payload(
            true,
            true,
            current_hz,
            None,
            saved_restore_hz.filter(|hz| *hz > BATTERY_REFRESH_HZ),
            format!(
                "Display is already below 60 Hz on battery at {current_hz} Hz; no refresh-rate change was applied."
            ),
        ));
    }

    set_refresh_rate_hz(BATTERY_REFRESH_HZ)?;
    let restore_hz = saved_restore_hz
        .filter(|hz| *hz > BATTERY_REFRESH_HZ)
        .unwrap_or(current_hz);

    Ok(payload(
        true,
        true,
        current_hz,
        Some(BATTERY_REFRESH_HZ),
        Some(restore_hz),
        format!(
            "Switched the display from {current_hz} Hz to 60 Hz for battery mode. AeroForge will restore {restore_hz} Hz when AC power returns."
        ),
    ))
}

fn sync_on_ac(
    current_hz: u32,
    saved_restore_hz: Option<u32>,
) -> Result<AutoRefreshRatePayload, DynError> {
    if let Some(restore_hz) = saved_restore_hz.filter(|hz| *hz > BATTERY_REFRESH_HZ) {
        if current_hz != restore_hz {
            set_refresh_rate_hz(restore_hz)?;
            return Ok(payload(
                true,
                false,
                current_hz,
                Some(restore_hz),
                None,
                format!(
                    "AC power detected. Restored the display refresh rate from {current_hz} Hz to {restore_hz} Hz."
                ),
            ));
        }
    }

    Ok(payload(
        true,
        false,
        current_hz,
        None,
        None,
        "AC power detected. Auto 60 Hz is armed for the next battery session.".into(),
    ))
}

fn payload(
    enabled: bool,
    on_battery: bool,
    current_hz: u32,
    applied_hz: Option<u32>,
    restore_hz: Option<u32>,
    detail: String,
) -> AutoRefreshRatePayload {
    AutoRefreshRatePayload {
        enabled,
        on_battery,
        current_hz,
        applied_hz,
        restore_hz,
        applied_at_unix: now_unix(),
        detail,
    }
}

fn current_display_mode() -> Result<DEVMODEW, DynError> {
    let mut mode = zeroed_devmode();
    let ok = unsafe { EnumDisplaySettingsW(null(), ENUM_CURRENT_SETTINGS, &mut mode) };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(mode)
}

fn current_refresh_rate_hz() -> Result<u32, DynError> {
    let mode = current_display_mode()?;
    if mode.dmDisplayFrequency == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Windows did not report the current display refresh rate.",
        )
        .into());
    }
    Ok(mode.dmDisplayFrequency)
}

fn set_refresh_rate_hz(refresh_hz: u32) -> Result<(), DynError> {
    let mut mode = current_display_mode()?;
    mode.dmFields |= DM_DISPLAYFREQUENCY;
    mode.dmDisplayFrequency = refresh_hz;

    let test = unsafe { ChangeDisplaySettingsW(&mode, CDS_TEST) };
    if test != DISP_CHANGE_SUCCESSFUL {
        return Err(format_display_change_error(test, refresh_hz, "test").into());
    }

    let applied = unsafe { ChangeDisplaySettingsW(&mode, 0) };
    if applied != DISP_CHANGE_SUCCESSFUL {
        return Err(format_display_change_error(applied, refresh_hz, "apply").into());
    }

    Ok(())
}

fn zeroed_devmode() -> DEVMODEW {
    let mut mode: DEVMODEW = unsafe { zeroed() };
    mode.dmSize = size_of::<DEVMODEW>() as u16;
    mode
}

fn format_display_change_error(code: DISP_CHANGE, refresh_hz: u32, phase: &str) -> String {
    format!(
        "Windows rejected the {phase} refresh-rate change to {refresh_hz} Hz with DISP_CHANGE code {code}."
    )
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
