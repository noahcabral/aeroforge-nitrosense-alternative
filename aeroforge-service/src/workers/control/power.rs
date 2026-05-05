use regex::Regex;
use std::process::Command;
use std::sync::OnceLock;

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::{
    acer_wmi::{
        apply_gaming_misc_setting, apply_gaming_profile, read_gaming_misc_setting,
        GAMING_PROFILE_BALANCED, GAMING_PROFILE_PERFORMANCE, GAMING_PROFILE_QUIET,
        GAMING_PROFILE_TURBO, MISC_SETTING_PLATFORM_PROFILE, MISC_SETTING_SUPPORTED_PROFILES,
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
        custom_base_profile: request.custom_base_profile.clone(),
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
            let firmware_detail = apply_acer_platform_profile_or_fallback(
                "quiet",
                u64::from(GAMING_PROFILE_QUIET),
                false,
                None,
            );
            let whisper_detail = match set_whisper_mode(true) {
                Ok(whisper) => {
                    format_whisper_detail("Applied direct NVIDIA Whisper quiet state", &whisper)
                }
                Err(error) => format!(
                    "NVIDIA Whisper quiet state was unavailable: {error}. Continuing with Windows processor policy."
                ),
            };
            Ok(AppliedOperatingMode {
                detail: format!("{firmware_detail} {whisper_detail}"),
            })
        }
        PowerProfileId::Balanced => {
            apply_acer_profile_with_whisper_clear(profile_id, GAMING_PROFILE_BALANCED, false, None)
        }
        PowerProfileId::Performance => apply_acer_profile_with_whisper_clear(
            profile_id,
            GAMING_PROFILE_PERFORMANCE,
            false,
            None,
        ),
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
    let firmware_detail = apply_acer_platform_profile_or_fallback(
        profile_label(profile_id),
        input,
        balanced_base_for_custom,
        custom_base_label,
    );

    let whisper_detail = match set_whisper_mode(false) {
        Ok(whisper) => format_whisper_detail("Cleared NVIDIA Whisper state", &whisper),
        Err(error) => format!("NVIDIA Whisper clear was unavailable: {error}."),
    };

    Ok(AppliedOperatingMode {
        detail: format!("{firmware_detail} {whisper_detail}"),
    })
}

fn apply_acer_platform_profile_or_fallback(
    profile_label: &str,
    input: u64,
    balanced_base_for_custom: bool,
    custom_base_label: Option<&'static str>,
) -> String {
    let supported_profiles_raw = read_misc_value_byte(MISC_SETTING_SUPPORTED_PROFILES);
    let current_before = read_misc_value_byte(MISC_SETTING_PLATFORM_PROFILE);
    let value = u8::try_from(input).unwrap_or_default();
    let supported_detail = describe_supported_profiles(&supported_profiles_raw, value);
    let before_detail = match current_before {
        Ok(Some(value)) => format!("Current platform profile before apply read as 0x{value:02X}."),
        Ok(None) => "Current platform profile before apply returned no gmOutput byte.".into(),
        Err(error) => format!("Current platform profile before apply was unavailable: {error}."),
    };

    let misc_result = apply_gaming_misc_setting(MISC_SETTING_PLATFORM_PROFILE, value);
    let after_misc = read_misc_value_byte(MISC_SETTING_PLATFORM_PROFILE);
    match misc_result {
        Ok(result) if misc_setting_output_accepted(result.output) => {
            let misc_confirmed = profile_readback_matches(&after_misc, value);
            let mode_label = if balanced_base_for_custom {
                custom_base_label.unwrap_or("performance")
            } else {
                profile_label
            };
            if misc_confirmed {
                return format!(
                    "Confirmed AcerGamingFunction {mode_label} platform profile with SetGamingMiscSetting(0x0B, 0x{value:02X}) gmOutput {:?}. {supported_detail} {before_detail} {}",
                    result.output,
                    describe_profile_readback("Current platform profile after misc-setting write", &after_misc)
                );
            }
            return format!(
                "AcerGamingFunction accepted SetGamingMiscSetting(0x0B, 0x{value:02X}) with gmOutput {:?}, but the follow-up platform-profile readback did not confirm the target. {supported_detail} {before_detail} {} {}",
                result.output,
                describe_profile_readback("Current platform profile after misc-setting write", &after_misc),
                apply_legacy_gaming_profile(
                    profile_label,
                    input,
                    value,
                    balanced_base_for_custom,
                    custom_base_label,
                )
            );
        }
        Ok(result) => {
            let fallback = apply_legacy_gaming_profile(
                profile_label,
                input,
                value,
                balanced_base_for_custom,
                custom_base_label,
            );
            return format!(
                "AcerGamingFunction rejected platform profile SetGamingMiscSetting(0x0B, 0x{value:02X}) with gmOutput {:?}. {supported_detail} {before_detail} {} {fallback}",
                result.output,
                describe_profile_readback("Current platform profile after misc-setting write", &after_misc)
            );
        }
        Err(error) => {
            let fallback = apply_legacy_gaming_profile(
                profile_label,
                input,
                value,
                balanced_base_for_custom,
                custom_base_label,
            );
            return format!(
                "AcerGamingFunction platform profile misc-setting write unavailable: {error}. {supported_detail} {before_detail} {} {fallback}",
                describe_profile_readback("Current platform profile after misc-setting write", &after_misc)
            );
        }
    }
}

fn apply_legacy_gaming_profile(
    profile_label: &str,
    input: u64,
    target_profile_value: u8,
    balanced_base_for_custom: bool,
    custom_base_label: Option<&'static str>,
) -> String {
    match apply_gaming_profile(input) {
        Ok(result) => {
            let after_legacy = read_misc_value_byte(MISC_SETTING_PLATFORM_PROFILE);
            let gm_output = result.output;
            let readback_confirmed = profile_readback_matches(&after_legacy, target_profile_value);
            if readback_confirmed || legacy_gaming_profile_output_accepted(gm_output) {
                if balanced_base_for_custom {
                    let verb = if readback_confirmed {
                        "Confirmed"
                    } else {
                        "Accepted but did not confirm"
                    };
                    format!(
                        "{verb} AcerGamingFunction {} base mode with SetGamingProfile({}) and gmOutput {:?} before layering the custom processor policy. {}",
                        custom_base_label.unwrap_or("performance"),
                        result.input,
                        gm_output,
                        describe_profile_readback("Current platform profile after legacy profile write", &after_legacy)
                    )
                } else {
                    let verb = if readback_confirmed {
                        "Confirmed"
                    } else {
                        "Accepted but did not confirm"
                    };
                    format!(
                        "{verb} AcerGamingFunction {} mode with SetGamingProfile({}) and gmOutput {:?}. {}",
                        profile_label,
                        result.input,
                        gm_output,
                        describe_profile_readback("Current platform profile after legacy profile write", &after_legacy)
                    )
                }
            } else if balanced_base_for_custom {
                format!(
                    "AcerGamingFunction rejected the {} base mode for Custom with SetGamingProfile({}) and gmOutput {:?}. {} Continuing with Windows processor policy only.",
                    custom_base_label.unwrap_or("performance"),
                    result.input,
                    gm_output,
                    describe_profile_readback("Current platform profile after legacy profile write", &after_legacy)
                )
            } else {
                format!(
                    "AcerGamingFunction rejected {} mode with SetGamingProfile({}) and gmOutput {:?}. {} Continuing with Windows processor policy only.",
                    profile_label,
                    result.input,
                    gm_output,
                    describe_profile_readback("Current platform profile after legacy profile write", &after_legacy)
                )
            }
        }
        Err(error) => format!(
            "AcerGamingFunction {} mode write was unavailable: {error}. Continuing with Windows processor policy only.",
            profile_label
        ),
    }
}

fn misc_setting_output_accepted(output: Option<u64>) -> bool {
    matches!(output, None | Some(0) | Some(1))
}

fn legacy_gaming_profile_output_accepted(output: Option<u64>) -> bool {
    matches!(output, None | Some(0) | Some(1))
}

fn describe_supported_profiles(
    result: &Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>>,
    target_profile_value: u8,
) -> String {
    match result {
        Ok(Some(mask)) => {
            let names = supported_profile_names(*mask);
            let target_state = if supported_profile_mask_contains(*mask, target_profile_value) {
                "advertised"
            } else {
                "not advertised"
            };
            format!(
                "SupportedProfiles raw bitmask 0x{mask:02X} advertises [{}]; target 0x{target_profile_value:02X} is {target_state}. AeroForge still records write/readback behavior because some Windows providers under-report this bitmask.",
                names.join(", ")
            )
        }
        Ok(None) => "SupportedProfiles probe returned no gmOutput byte.".into(),
        Err(error) => format!("SupportedProfiles probe unavailable: {error}."),
    }
}

fn supported_profile_mask_contains(mask: u8, profile: u8) -> bool {
    profile < 8 && (mask & (1u8 << profile)) != 0
}

fn supported_profile_names(mask: u8) -> Vec<&'static str> {
    let mut names = Vec::new();
    for (bit, name) in [
        (0, "quiet"),
        (1, "balanced"),
        (4, "performance"),
        (5, "turbo"),
        (6, "eco"),
    ] {
        if (mask & (1u8 << bit)) != 0 {
            names.push(name);
        }
    }
    if names.is_empty() {
        names.push("none");
    }
    names
}

fn read_misc_value_byte(
    setting: u8,
) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
    Ok(read_gaming_misc_setting(setting)?
        .output
        .map(|value| (value & 0xFF) as u8))
}

fn profile_readback_matches(
    result: &Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>>,
    expected: u8,
) -> bool {
    matches!(result, Ok(Some(value)) if *value == expected)
}

fn describe_profile_readback(
    label: &str,
    result: &Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>>,
) -> String {
    match result {
        Ok(Some(value)) => format!("{label} read as 0x{value:02X}."),
        Ok(None) => format!("{label} returned no gmOutput byte."),
        Err(error) => format!("{label} was unavailable: {error}."),
    }
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
