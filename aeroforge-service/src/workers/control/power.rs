use regex::Regex;
use std::process::Command;
use std::sync::OnceLock;

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::{
    acer_wmi::{
        apply_gaming_profile, GAMING_PROFILE_BALANCED, GAMING_PROFILE_PERFORMANCE,
        GAMING_PROFILE_TURBO,
    },
    models::{
        AppliedPowerProfileSnapshot, ApplyPowerProfileRequest, CustomPowerBaseId, PowerProfileId,
        ProcessorStateReadback, ProcessorStateSettings,
    },
    nvapi_whisper::{set_whisper_mode, NvApiWhisperResult},
};

const SUB_PROCESSOR: &str = "SUB_PROCESSOR";
const PROCTHROTTLEMIN: &str = "PROCTHROTTLEMIN";
const PROCTHROTTLEMAX: &str = "PROCTHROTTLEMAX";
const SCHEME_CURRENT: &str = "SCHEME_CURRENT";

pub fn apply_power_profile(
    paths: &ServicePaths,
    request: ApplyPowerProfileRequest,
) -> Result<AppliedPowerProfileSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let profile_id = request.profile_id.clone();
    let processor_state = sanitize_processor_state(request.processor_state)?;
    let operating_mode = apply_operating_mode(&profile_id, request.custom_base_profile.as_ref())?;
    let profile_label = profile_label(&profile_id);

    write_log_line(
        &paths.component_log("control-power"),
        "INFO",
        &format!(
            "Applying power profile {} with processor state min {} / max {}.",
            profile_label, processor_state.min_percent, processor_state.max_percent
        ),
    )?;

    apply_scheme_value(
        "setacvalueindex",
        PROCTHROTTLEMIN,
        processor_state.min_percent,
    )?;
    apply_scheme_value(
        "setacvalueindex",
        PROCTHROTTLEMAX,
        processor_state.max_percent,
    )?;
    apply_scheme_value(
        "setdcvalueindex",
        PROCTHROTTLEMIN,
        processor_state.min_percent,
    )?;
    apply_scheme_value(
        "setdcvalueindex",
        PROCTHROTTLEMAX,
        processor_state.max_percent,
    )?;
    run_powercfg(&["/setactive", SCHEME_CURRENT])?;
    let readback = read_processor_state_readback()?;
    let drift_detected = readback.ac.min_percent != processor_state.min_percent
        || readback.ac.max_percent != processor_state.max_percent
        || readback.dc.min_percent != processor_state.min_percent
        || readback.dc.max_percent != processor_state.max_percent;

    let detail = format!(
        "{} Windows processor policy requested min {} / max {} on the active scheme. Read back AC {} / {} and DC {} / {}.{}",
        operating_mode.detail,
        processor_state.min_percent,
        processor_state.max_percent,
        readback.ac.min_percent,
        readback.ac.max_percent,
        readback.dc.min_percent,
        readback.dc.max_percent,
        if drift_detected {
            " Windows reported a different processor policy than the requested values."
        } else {
            ""
        }
    );

    write_log_line(&paths.component_log("control-power"), "INFO", &detail)?;

    Ok(AppliedPowerProfileSnapshot {
        profile_id,
        processor_state,
        readback,
        drift_detected,
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn sanitize_processor_state(
    processor_state: ProcessorStateSettings,
) -> Result<ProcessorStateSettings, Box<dyn std::error::Error + Send + Sync>> {
    let min_percent = processor_state.min_percent.clamp(5, 100);
    let max_percent = processor_state.max_percent.clamp(5, 100);

    if min_percent > max_percent {
        return Err(format!(
            "Processor minimum {} cannot exceed maximum {}.",
            min_percent, max_percent
        )
        .into());
    }

    Ok(ProcessorStateSettings {
        min_percent,
        max_percent,
    })
}

fn apply_operating_mode(
    profile_id: &PowerProfileId,
    custom_base_profile: Option<&CustomPowerBaseId>,
) -> Result<AppliedOperatingMode, Box<dyn std::error::Error + Send + Sync>> {
    match profile_id {
        PowerProfileId::BatteryGuard => {
            let whisper = set_whisper_mode(true)?;
            Ok(AppliedOperatingMode {
                detail: format_whisper_detail(
                    "Applied direct NVIDIA Whisper quiet state",
                    &whisper,
                ),
            })
        }
        PowerProfileId::Balanced => {
            apply_acer_profile_with_whisper_clear(
                profile_id,
                GAMING_PROFILE_BALANCED,
                false,
                None,
            )
        }
        PowerProfileId::Performance => {
            apply_acer_profile_with_whisper_clear(
                profile_id,
                GAMING_PROFILE_PERFORMANCE,
                false,
                None,
            )
        }
        PowerProfileId::Turbo => {
            apply_acer_profile_with_whisper_clear(profile_id, GAMING_PROFILE_TURBO, false, None)
        }
        PowerProfileId::Custom => {
            let custom_base = custom_base_profile
                .cloned()
                .unwrap_or(CustomPowerBaseId::Performance);
            let (input, label) = custom_base_profile_details(&custom_base);
            apply_acer_profile_with_whisper_clear(profile_id, input, true, Some(label))
        }
    }
}

fn profile_label(profile_id: &PowerProfileId) -> &'static str {
    match profile_id {
        PowerProfileId::BatteryGuard => "quiet",
        PowerProfileId::Balanced => "balanced",
        PowerProfileId::Performance => "performance",
        PowerProfileId::Turbo => "turbo",
        PowerProfileId::Custom => "custom",
    }
}

struct AppliedOperatingMode {
    detail: String,
}

fn apply_acer_profile_with_whisper_clear(
    profile_id: &PowerProfileId,
    input: u64,
    balanced_base_for_custom: bool,
    custom_base_label: Option<&'static str>,
) -> Result<AppliedOperatingMode, Box<dyn std::error::Error + Send + Sync>> {
    let result = apply_gaming_profile(input)?;
    let gm_output = result.output.ok_or_else(|| {
        format!(
            "AcerGamingFunction {} did not return gmOutput for {} mode.",
            result.method,
            profile_label(profile_id)
        )
    })?;

    if gm_output != 1 {
        return Err(format!(
            "{} mode was rejected by direct AcerGamingFunction control with gmOutput {}.",
            profile_label(profile_id),
            gm_output
        )
        .into());
    }

    let whisper = set_whisper_mode(false).map_err(|error| {
        format!(
            "{} mode reached its AcerGamingFunction base but failed to clear NVIDIA Whisper state: {}",
            profile_label(profile_id),
            error
        )
    })?;

    let firmware_detail = if balanced_base_for_custom {
        format!(
            "Applied AcerGamingFunction {} base mode with SetGamingProfile({}) and gmOutput {} before layering the custom processor policy.",
            custom_base_label.unwrap_or("performance"),
            result.input,
            gm_output
        )
    } else {
        format!(
            "Applied AcerGamingFunction {} mode with SetGamingProfile({}) and gmOutput {}.",
            profile_label(profile_id),
            result.input,
            gm_output
        )
    };

    Ok(AppliedOperatingMode {
        detail: format!(
            "{firmware_detail} {}",
            format_whisper_detail("Cleared NVIDIA Whisper state", &whisper)
        ),
    })
}

fn custom_base_profile_details(custom_base: &CustomPowerBaseId) -> (u64, &'static str) {
    match custom_base {
        CustomPowerBaseId::Balanced => (GAMING_PROFILE_BALANCED, "balanced"),
        CustomPowerBaseId::Performance => (GAMING_PROFILE_PERFORMANCE, "performance"),
        CustomPowerBaseId::Turbo => (GAMING_PROFILE_TURBO, "turbo"),
    }
}

fn format_whisper_detail(prefix: &str, result: &NvApiWhisperResult) -> String {
    format!(
        "{prefix} with NVAPI init 0x{:08X} status {} and hidden setter 0x{:08X} status {} (enabled={}).",
        result.init_candidate_id,
        result.init_status,
        result.hidden_id,
        result.status,
        if result.enabled { "true" } else { "false" }
    )
}

fn apply_scheme_value(
    action: &str,
    setting: &str,
    value: u8,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_powercfg(&[
        &format!("/{action}"),
        SCHEME_CURRENT,
        SUB_PROCESSOR,
        setting,
        &value.to_string(),
    ])
}

fn run_powercfg(arguments: &[&str]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let output = run_powercfg_output(arguments)?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("powercfg exited with status {}", output.status)
    };

    Err(format!("powercfg {} failed: {}", arguments.join(" "), detail).into())
}

fn run_powercfg_output(
    arguments: &[&str],
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Command::new("powercfg").args(arguments).output()?)
}

fn read_processor_state_readback(
) -> Result<ProcessorStateReadback, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ProcessorStateReadback {
        ac: ProcessorStateSettings {
            min_percent: query_scheme_value(PROCTHROTTLEMIN, "Current AC Power Setting Index")?,
            max_percent: query_scheme_value(PROCTHROTTLEMAX, "Current AC Power Setting Index")?,
        },
        dc: ProcessorStateSettings {
            min_percent: query_scheme_value(PROCTHROTTLEMIN, "Current DC Power Setting Index")?,
            max_percent: query_scheme_value(PROCTHROTTLEMAX, "Current DC Power Setting Index")?,
        },
    })
}

fn query_scheme_value(
    setting: &str,
    label: &str,
) -> Result<u8, Box<dyn std::error::Error + Send + Sync>> {
    let output = run_powercfg_output(&["/q", SCHEME_CURRENT, SUB_PROCESSOR, setting])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("powercfg exited with status {}", output.status)
        };
        return Err(format!("powercfg /q {} failed: {}", setting, detail).into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let regex = current_index_regex();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.contains(label) {
            continue;
        }

        if let Some(captures) = regex.captures(trimmed) {
            let raw = captures
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let parsed = u32::from_str_radix(raw, 16)?;
            return u8::try_from(parsed).map_err(|_| {
                format!("Readback value {} for {} exceeds u8 range", parsed, setting).into()
            });
        }
    }

    Err(format!(
        "Could not find {} in powercfg readback for {}",
        label, setting
    )
    .into())
}

fn current_index_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"0x([0-9A-Fa-f]+)").expect("valid powercfg regex"))
}
