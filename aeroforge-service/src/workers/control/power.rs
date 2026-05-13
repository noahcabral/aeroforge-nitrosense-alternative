use regex::Regex;
use std::{env, process::Command, sync::OnceLock, thread, time::Duration};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::{
    acer_hid::{self, SystemUsageMode},
    acer_wmi::{
        apply_gaming_misc_setting, apply_gaming_profile, decode_gm_output_byte,
        read_gaming_misc_setting, GAMING_PROFILE_BALANCED, GAMING_PROFILE_PERFORMANCE,
        GAMING_PROFILE_QUIET, GAMING_PROFILE_TURBO, MISC_SETTING_PLATFORM_PROFILE,
        MISC_SETTING_SUPPORTED_PROFILES,
    },
    models::{
        AppliedPowerProfileSnapshot, ApplyPowerProfileRequest, CustomPowerBaseId, PowerProfileId,
        ProcessorStateReadback, ProcessorStateSettings,
    },
    nvapi_whisper::{set_whisper_mode, NvApiWhisperResult},
    nvidia_power, rapl_power,
};

const SUB_PROCESSOR: &str = "SUB_PROCESSOR";
const PROCTHROTTLEMIN: &str = "PROCTHROTTLEMIN";
const PROCTHROTTLEMAX: &str = "PROCTHROTTLEMAX";
const SCHEME_CURRENT: &str = "SCHEME_CURRENT";

#[derive(Clone, Copy)]
enum PowercfgCurrentIndex {
    Ac,
    Dc,
}

pub fn apply_power_profile(
    paths: &ServicePaths,
    request: ApplyPowerProfileRequest,
) -> Result<AppliedPowerProfileSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let profile_id = request.profile_id.clone();
    let custom_base_profile = request.custom_base_profile.clone();
    let processor_state_control_enabled = request.processor_state_control_enabled;
    let processor_state = sanitize_processor_state(request.processor_state)?;
    let power_before = nvidia_power::read_power_readback(paths);
    let operating_mode = apply_operating_mode(paths, &profile_id, custom_base_profile.as_ref())?;
    let profile_label = profile_label(&profile_id);

    write_log_line(
        &paths.component_log("control-power"),
        "INFO",
        &format!(
            "Applying power profile {} with processor state min {} / max {} (processor state writes {}).",
            profile_label,
            processor_state.min_percent,
            processor_state.max_percent,
            if processor_state_control_enabled {
                "enabled"
            } else {
                "disabled"
            }
        ),
    )?;

    if processor_state_control_enabled {
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
    }

    let readback = read_processor_state_readback()?;
    let drift_detected = processor_state_control_enabled
        && (readback.ac.min_percent != processor_state.min_percent
            || readback.ac.max_percent != processor_state.max_percent
            || readback.dc.min_percent != processor_state.min_percent
            || readback.dc.max_percent != processor_state.max_percent);
    let cpu_power_limit_detail =
        rapl_power::apply_profile_package_limit(paths, &profile_id, custom_base_profile.as_ref());

    thread::sleep(Duration::from_millis(500));
    let power_after = nvidia_power::read_power_readback(paths);
    let power_detail = nvidia_power::format_power_limit_delta(&power_before, &power_after);

    let processor_detail = if processor_state_control_enabled {
        format!(
            "Windows processor policy requested min {} / max {} on the active scheme. Read back AC {} / {} and DC {} / {}.{}",
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
        )
    } else {
        format!(
            "Windows processor min/max policy changes were skipped by AeroForge settings. Current readback AC {} / {} and DC {} / {}.",
            readback.ac.min_percent,
            readback.ac.max_percent,
            readback.dc.min_percent,
            readback.dc.max_percent
        )
    };

    let detail = format!(
        "{} {} {} {}",
        operating_mode.detail, processor_detail, cpu_power_limit_detail, power_detail
    );

    write_log_line(&paths.component_log("control-power"), "INFO", &detail)?;

    Ok(AppliedPowerProfileSnapshot {
        profile_id,
        processor_state,
        custom_base_profile,
        processor_state_control_enabled,
        readback,
        drift_detected,
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn sanitize_processor_state(
    processor_state: ProcessorStateSettings,
) -> Result<ProcessorStateSettings, Box<dyn std::error::Error + Send + Sync>> {
    let min_percent = processor_state.min_percent.clamp(0, 100);
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
    paths: &ServicePaths,
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
            let direct_hid_detail = apply_direct_hid_mode_detail(SystemUsageMode::Quiet);
            let whisper_detail = apply_whisper_mode_detail(
                paths,
                true,
                "Applied direct NVIDIA Whisper quiet state",
                "NVIDIA Whisper quiet state was unavailable",
            );
            Ok(AppliedOperatingMode {
                detail: format!("{firmware_detail} {direct_hid_detail} {whisper_detail}"),
            })
        }
        PowerProfileId::Balanced => apply_acer_profile_with_whisper_clear(
            paths,
            profile_id,
            GAMING_PROFILE_BALANCED,
            false,
            None,
        ),
        PowerProfileId::Performance => apply_acer_profile_with_whisper_clear(
            paths,
            profile_id,
            GAMING_PROFILE_PERFORMANCE,
            false,
            None,
        ),
        PowerProfileId::Turbo => apply_acer_profile_with_whisper_clear(
            paths,
            profile_id,
            GAMING_PROFILE_TURBO,
            false,
            None,
        ),
        PowerProfileId::Custom => {
            let custom_base = custom_base_profile
                .cloned()
                .unwrap_or(CustomPowerBaseId::Performance);
            let (input, label) = custom_base_profile_details(&custom_base);
            apply_acer_profile_with_whisper_clear(paths, profile_id, input, true, Some(label))
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
    paths: &ServicePaths,
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
    let direct_hid_mode = direct_hid_mode_for_profile_input(input);
    let direct_hid_detail = direct_hid_mode
        .map(apply_direct_hid_mode_detail)
        .unwrap_or_else(|| {
            "Direct Acer HID system-usage mode write skipped: no mode mapping.".into()
        });

    let whisper_detail = apply_whisper_mode_detail(
        paths,
        false,
        "Cleared NVIDIA Whisper state",
        "NVIDIA Whisper clear was unavailable",
    );
    let turbo_oc_detail = if direct_hid_mode == Some(SystemUsageMode::Turbo) {
        apply_turbo_oc_profile_hint_detail()
    } else {
        "Direct Acer HID turbo OC-profile hint skipped for non-turbo mode.".into()
    };

    Ok(AppliedOperatingMode {
        detail: format!("{firmware_detail} {direct_hid_detail} {whisper_detail} {turbo_oc_detail}"),
    })
}

fn direct_hid_mode_for_profile_input(input: u64) -> Option<SystemUsageMode> {
    match input {
        GAMING_PROFILE_BALANCED => Some(SystemUsageMode::Normal),
        GAMING_PROFILE_PERFORMANCE => Some(SystemUsageMode::Performance),
        GAMING_PROFILE_TURBO => Some(SystemUsageMode::Turbo),
        _ => None,
    }
}

fn apply_direct_hid_mode_detail(mode: SystemUsageMode) -> String {
    match acer_hid::apply_system_usage_mode(mode) {
        Ok(result) => format!(
            "Direct Acer HID system-usage mode write applied {} with request {}{}.",
            result.label,
            result.request_prefix,
            result
                .response_prefix
                .as_ref()
                .map(|response| format!(" and response {response}"))
                .unwrap_or_default()
        ),
        Err(error) => format!(
            "Direct Acer HID system-usage mode write unavailable for {}: {error}.",
            mode.label()
        ),
    }
}

fn apply_turbo_oc_profile_hint_detail() -> String {
    match acer_hid::apply_turbo_oc_profile_hint() {
        Ok(results) => {
            let details = results
                .iter()
                .map(|result| {
                    format!(
                        "{} {} request {}",
                        result.action, result.label, result.request_prefix
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!("Direct Acer HID turbo OC-profile hint applied: {details}.")
        }
        Err(error) => format!("Direct Acer HID turbo OC-profile hint unavailable: {error}."),
    }
}

fn apply_acer_platform_profile_or_fallback(
    profile_label: &str,
    input: u64,
    balanced_base_for_custom: bool,
    custom_base_label: Option<&'static str>,
) -> String {
    let value = u8::try_from(input).unwrap_or_default();
    let mode_label = if balanced_base_for_custom {
        custom_base_label.unwrap_or("performance")
    } else {
        profile_label
    };

    match apply_gaming_misc_setting(MISC_SETTING_PLATFORM_PROFILE, value) {
        Ok(result) if misc_setting_output_accepted(result.output, value) => {
            if profile_readback_enabled() {
                let supported_profiles_raw = read_misc_value_byte(MISC_SETTING_SUPPORTED_PROFILES);
                let after_misc = read_misc_value_byte(MISC_SETTING_PLATFORM_PROFILE);
                let supported_detail = describe_supported_profiles(&supported_profiles_raw, value);
                if profile_readback_matches(&after_misc, value) {
                    return format!(
                        "Confirmed AcerGamingFunction {mode_label} platform profile with SetGamingMiscSetting(0x0B, 0x{value:02X}) gmOutput {:?}. {supported_detail} {}",
                        result.output,
                        describe_profile_readback(
                            "Current platform profile after misc-setting write",
                            &after_misc,
                        )
                    );
                }
                return format!(
                    "Applied AcerGamingFunction {mode_label} platform profile with SetGamingMiscSetting(0x0B, 0x{value:02X}) gmOutput {:?}; readback did not confirm the target. {supported_detail} {} Legacy fallback skipped because the primary write was accepted.",
                    result.output,
                    describe_profile_readback(
                        "Current platform profile after misc-setting write",
                        &after_misc,
                    )
                );
            }
            return format!(
                "Applied AcerGamingFunction {mode_label} platform profile with SetGamingMiscSetting(0x0B, 0x{value:02X}) gmOutput {:?}. Platform profile readback skipped for responsive profile switching; set AEROFORGE_PROFILE_READBACK=1 for diagnostics.",
                result.output
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
                "AcerGamingFunction rejected platform profile SetGamingMiscSetting(0x0B, 0x{value:02X}) with gmOutput {:?}. Platform readback skipped before fallback for responsive profile switching. {fallback}",
                result.output
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
                "AcerGamingFunction platform profile misc-setting write unavailable: {error}. Platform readback skipped before fallback for responsive profile switching. {fallback}"
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
            let after_legacy = profile_readback_enabled()
                .then(|| read_misc_value_byte(MISC_SETTING_PLATFORM_PROFILE));
            let gm_output = result.output;
            let readback_confirmed = after_legacy
                .as_ref()
                .map(|readback| profile_readback_matches(readback, target_profile_value))
                .unwrap_or(false);
            let readback_detail = after_legacy
                .as_ref()
                .map(|readback| {
                    describe_profile_readback(
                        "Current platform profile after legacy profile write",
                        readback,
                    )
                })
                .unwrap_or_else(|| {
                    "Current platform profile after legacy profile write readback skipped for responsive profile switching.".into()
                });
            if readback_confirmed || legacy_gaming_profile_output_accepted(gm_output) {
                if balanced_base_for_custom {
                    let verb = if readback_confirmed {
                        "Confirmed"
                    } else if profile_readback_enabled() {
                        "Accepted but did not confirm"
                    } else {
                        "Accepted"
                    };
                    format!(
                        "{verb} AcerGamingFunction {} base mode with SetGamingProfile({}) and gmOutput {:?} before layering the custom processor policy. {}",
                        custom_base_label.unwrap_or("performance"),
                        result.input,
                        gm_output,
                        readback_detail
                    )
                } else {
                    let verb = if readback_confirmed {
                        "Confirmed"
                    } else if profile_readback_enabled() {
                        "Accepted but did not confirm"
                    } else {
                        "Accepted"
                    };
                    format!(
                        "{verb} AcerGamingFunction {} mode with SetGamingProfile({}) and gmOutput {:?}. {}",
                        profile_label,
                        result.input,
                        gm_output,
                        readback_detail
                    )
                }
            } else if balanced_base_for_custom {
                format!(
                    "AcerGamingFunction rejected the {} base mode for Custom with SetGamingProfile({}) and gmOutput {:?}. {} Continuing with Windows processor policy only.",
                    custom_base_label.unwrap_or("performance"),
                    result.input,
                    gm_output,
                    readback_detail
                )
            } else {
                format!(
                    "AcerGamingFunction rejected {} mode with SetGamingProfile({}) and gmOutput {:?}. {} Continuing with Windows processor policy only.",
                    profile_label,
                    result.input,
                    gm_output,
                    readback_detail
                )
            }
        }
        Err(error) => format!(
            "AcerGamingFunction {} mode write was unavailable: {error}. Continuing with Windows processor policy only.",
            profile_label
        ),
    }
}

fn misc_setting_output_accepted(output: Option<u64>, expected: u8) -> bool {
    match output {
        None | Some(0) | Some(1) => true,
        Some(value) => decode_gm_output_byte(value) == expected,
    }
}

fn legacy_gaming_profile_output_accepted(output: Option<u64>) -> bool {
    matches!(output, None | Some(0) | Some(1))
}

fn profile_readback_enabled() -> bool {
    env::var("AEROFORGE_PROFILE_READBACK")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
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
        .map(decode_gm_output_byte))
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

fn apply_whisper_mode_detail(
    paths: &ServicePaths,
    enabled: bool,
    success_prefix: &str,
    error_prefix: &str,
) -> String {
    if !nvidia_power::nvidia_access_enabled(paths) {
        return "NVIDIA Whisper control skipped because NVIDIA telemetry/control access is disabled; this avoids waking the dGPU during power-mode changes.".into();
    }

    match set_whisper_mode(enabled) {
        Ok(whisper) => format_whisper_detail(success_prefix, &whisper),
        Err(error) => format!("{error_prefix}: {error}."),
    }
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
            min_percent: query_scheme_value(
                PROCTHROTTLEMIN,
                "Current AC Power Setting Index",
                PowercfgCurrentIndex::Ac,
            )?,
            max_percent: query_scheme_value(
                PROCTHROTTLEMAX,
                "Current AC Power Setting Index",
                PowercfgCurrentIndex::Ac,
            )?,
        },
        dc: ProcessorStateSettings {
            min_percent: query_scheme_value(
                PROCTHROTTLEMIN,
                "Current DC Power Setting Index",
                PowercfgCurrentIndex::Dc,
            )?,
            max_percent: query_scheme_value(
                PROCTHROTTLEMAX,
                "Current DC Power Setting Index",
                PowercfgCurrentIndex::Dc,
            )?,
        },
    })
}

fn query_scheme_value(
    setting: &str,
    label: &str,
    current_index: PowercfgCurrentIndex,
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
    if let Some(value) = parse_labeled_powercfg_value(&stdout, setting, label)? {
        return Ok(value);
    }

    if let Some(value) = parse_scoped_powercfg_value(&stdout, setting, current_index)? {
        return Ok(value);
    }

    Err(format!(
        "Could not find {} in powercfg readback for {}. The output did not contain enough numeric indexes for locale-neutral fallback parsing.",
        label, setting
    )
    .into())
}

fn parse_labeled_powercfg_value(
    stdout: &str,
    setting: &str,
    label: &str,
) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
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
            return Ok(Some(percent_from_powercfg_hex(parsed, setting)?));
        }
    }
    Ok(None)
}

fn parse_scoped_powercfg_value(
    stdout: &str,
    setting: &str,
    current_index: PowercfgCurrentIndex,
) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let regex = current_index_regex();
    let mut all_values = Vec::new();
    let mut setting_values = Vec::new();
    let mut in_target_setting = false;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.contains("Alias GUID:") {
            if in_target_setting && !setting_values.is_empty() && !trimmed.contains(setting) {
                break;
            }
            if trimmed.contains(setting) {
                in_target_setting = true;
                setting_values.clear();
                continue;
            }
        }

        if in_target_setting && trimmed.is_empty() && !setting_values.is_empty() {
            break;
        }

        for captures in regex.captures_iter(trimmed) {
            let raw = captures
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let parsed = u32::from_str_radix(raw, 16)?;
            all_values.push(parsed);
            if in_target_setting {
                setting_values.push(parsed);
            }
        }
    }

    let values = if setting_values.len() >= 2 {
        &setting_values
    } else {
        &all_values
    };
    if values.len() < 2 {
        return Ok(None);
    }

    let parsed = match current_index {
        PowercfgCurrentIndex::Ac => values[values.len() - 2],
        PowercfgCurrentIndex::Dc => values[values.len() - 1],
    };
    Ok(Some(percent_from_powercfg_hex(parsed, setting)?))
}

fn percent_from_powercfg_hex(
    parsed: u32,
    setting: &str,
) -> Result<u8, Box<dyn std::error::Error + Send + Sync>> {
    u8::try_from(parsed)
        .map_err(|_| format!("Readback value {} for {} exceeds u8 range", parsed, setting).into())
}

fn current_index_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"0x([0-9A-Fa-f]+)").expect("valid powercfg regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_localized_processor_state_indexes() {
        let output = r#"
    GUID de Configuração de Energia: 893dee8e-2bef-41e0-89c6-b55d0929964c  (Estado de desempenho mínimo)
      Alias GUID: PROCTHROTTLEMIN
      Configuração Mínima Possível: 0x00000000
      Configuração Máxima Possível: 0x00000064
      Incremento de Configurações Possíveis: 0x00000001
      Unidades de Configurações Possíveis: %
    Índice de Configurações de Correntes Alternadas Atuais: 0x00000023
    Índice de Configurações de Correntes Contínuas Atuais: 0x0000002D
"#;

        assert_eq!(
            parse_scoped_powercfg_value(output, PROCTHROTTLEMIN, PowercfgCurrentIndex::Ac).unwrap(),
            Some(35)
        );
        assert_eq!(
            parse_scoped_powercfg_value(output, PROCTHROTTLEMIN, PowercfgCurrentIndex::Dc).unwrap(),
            Some(45)
        );
    }

    #[test]
    fn keeps_english_label_fast_path() {
        let output = "Current AC Power Setting Index: 0x00000058";
        assert_eq!(
            parse_labeled_powercfg_value(output, PROCTHROTTLEMAX, "Current AC Power Setting Index")
                .unwrap(),
            Some(88)
        );
    }
}
