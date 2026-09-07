use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
};

use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use super::models::PowerSnapshot;

const SYSTEM_BATTERY_STATE_LEVEL: u32 = 5;
const STATUS_SUCCESS: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SystemBatteryState {
    ac_on_line: u8,
    battery_present: u8,
    charging: u8,
    discharging: u8,
    spare1: [u8; 3],
    tag: u8,
    max_capacity: u32,
    remaining_capacity: u32,
    rate: u32,
    estimated_time: u32,
    default_alert1: u32,
    default_alert2: u32,
}

extern "system" {
    fn CallNtPowerInformation(
        information_level: u32,
        input_buffer: *mut c_void,
        input_buffer_length: u32,
        output_buffer: *mut c_void,
        output_buffer_length: u32,
    ) -> i32;
}

pub fn read_power_snapshot() -> Result<PowerSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let mut status = unsafe { zeroed::<SYSTEM_POWER_STATUS>() };
    let ok = unsafe { GetSystemPowerStatus(&mut status) };

    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let battery_percent = if status.BatteryLifePercent == u8::MAX {
        0
    } else {
        status.BatteryLifePercent
    };
    let ac_plugged_in = status.ACLineStatus == 1;
    let battery_life_remaining_sec = if ac_plugged_in {
        None
    } else {
        read_precise_battery_estimate().or_else(|| {
            (status.BatteryLifeTime != u32::MAX && status.BatteryLifeTime > 0)
                .then_some(status.BatteryLifeTime)
        })
    };

    Ok(PowerSnapshot {
        battery_percent,
        battery_life_remaining_sec,
        ac_plugged_in,
    })
}

fn read_precise_battery_estimate() -> Option<u32> {
    let mut state = SystemBatteryState::default();
    let status = unsafe {
        CallNtPowerInformation(
            SYSTEM_BATTERY_STATE_LEVEL,
            std::ptr::null_mut(),
            0,
            (&mut state as *mut SystemBatteryState).cast(),
            size_of::<SystemBatteryState>() as u32,
        )
    };

    if status != STATUS_SUCCESS || state.battery_present == 0 || state.discharging == 0 {
        return None;
    }

    if state.estimated_time != u32::MAX && state.estimated_time > 0 {
        return Some(state.estimated_time);
    }

    if state.rate > 0 && state.remaining_capacity > 0 {
        let estimated_seconds =
            ((state.remaining_capacity as u64) * 3600 / (state.rate as u64)).min(u32::MAX as u64);
        if estimated_seconds > 0 {
            return Some(estimated_seconds as u32);
        }
    }

    None
}
