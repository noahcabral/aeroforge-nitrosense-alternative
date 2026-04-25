use std::{
    fs, io,
    path::Path,
    ptr::null_mut,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};
use windows_sys::Win32::{
    Graphics::Gdi::{GetDC, ReleaseDC},
    UI::ColorSystem::SetDeviceGammaRamp,
};

type DynError = Box<dyn std::error::Error + Send + Sync>;

const SETTINGS_PATH: &str = r"C:\ProgramData\Acer\QA\settings.json";
const DEFAULT_ENABLED_GAIN_ID: u8 = 3;
const MAX_GAMMA_VALUE: u16 = 65280;

pub struct BlueLightApplyPayload {
    pub enabled: bool,
    pub gain_id: u8,
    pub applied_at_unix: u64,
    pub detail: String,
}

pub fn sync_saved_state(enabled: bool) -> Result<BlueLightApplyPayload, DynError> {
    apply_blue_light_filter(enabled)
}

pub fn apply_blue_light_filter(enabled: bool) -> Result<BlueLightApplyPayload, DynError> {
    let gain_id = resolve_gain_id(enabled)?;
    persist_gain_id(gain_id)?;
    apply_gamma_ramp(gain_id)?;

    Ok(BlueLightApplyPayload {
        enabled,
        gain_id,
        applied_at_unix: now_unix(),
        detail: if enabled {
            format!(
                "Applied the Acer-style blue light gamma ramp at GainID {} and updated Quick Access settings.",
                gain_id
            )
        } else {
            "Restored the neutral Acer blue light gamma ramp and updated Quick Access settings."
                .into()
        },
    })
}

fn resolve_gain_id(enabled: bool) -> Result<u8, DynError> {
    if !enabled {
        return Ok(0);
    }

    match read_gain_id()? {
        Some(gain_id @ 1..=4) => Ok(gain_id),
        _ => Ok(DEFAULT_ENABLED_GAIN_ID),
    }
}

fn read_gain_id() -> Result<Option<u8>, DynError> {
    let path = Path::new(SETTINGS_PATH);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)?;
    let parsed = serde_json::from_str::<Value>(&raw)?;
    Ok(parsed
        .get("BluelightShield")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok()))
}

fn persist_gain_id(gain_id: u8) -> Result<(), DynError> {
    let path = Path::new(SETTINGS_PATH);

    let mut root = if path.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(path)?)?
    } else {
        Value::Object(Map::new())
    };

    let object = root
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Quick Access settings.json was not a JSON object."))?;
    object.insert("BluelightShield".into(), Value::from(gain_id));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

fn apply_gamma_ramp(gain_id: u8) -> Result<(), DynError> {
    let ramp_profile = gain_profile(gain_id)?;
    let mut ramp = [0u16; 3 * 256];

    fill_channel(&mut ramp[0..256], ramp_profile.red_scale);
    fill_channel(&mut ramp[256..512], ramp_profile.green_scale);
    fill_channel(&mut ramp[512..768], ramp_profile.blue_scale);

    unsafe {
        let device_context = GetDC(null_mut());
        if device_context.is_null() {
            return Err(io::Error::last_os_error().into());
        }

        let applied = SetDeviceGammaRamp(device_context, ramp.as_mut_ptr().cast()) != 0;
        ReleaseDC(null_mut(), device_context);

        if !applied {
            return Err(io::Error::last_os_error().into());
        }
    }

    Ok(())
}

fn fill_channel(channel: &mut [u16], scale: f32) {
    for (index, value) in channel.iter_mut().enumerate() {
        let base = (index as u32) * 256;
        let scaled = ((base as f32) * scale).trunc() as u32;
        *value = scaled.min(MAX_GAMMA_VALUE as u32) as u16;
    }
}

fn gain_profile(gain_id: u8) -> Result<GainProfile, DynError> {
    match gain_id {
        0 => Ok(GainProfile {
            red_scale: 1.0,
            green_scale: 1.0,
            blue_scale: 1.0,
        }),
        1 => Ok(GainProfile {
            red_scale: 1.0,
            green_scale: 1.0,
            blue_scale: 0.85,
        }),
        2 => Ok(GainProfile {
            red_scale: 1.0,
            green_scale: 1.0,
            blue_scale: 0.70,
        }),
        3 => Ok(GainProfile {
            red_scale: 1.0,
            green_scale: 1.0,
            blue_scale: 0.60,
        }),
        4 => Ok(GainProfile {
            red_scale: 1.06,
            green_scale: 1.0,
            blue_scale: 0.50,
        }),
        _ => Err(io::Error::other(format!(
            "Unsupported Acer blue light GainID {gain_id}. Valid GainIDs are 0 through 4."
        ))
        .into()),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct GainProfile {
    red_scale: f32,
    green_scale: f32,
    blue_scale: f32,
}
