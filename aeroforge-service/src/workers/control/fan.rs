use serde_json::{json, Value};
use std::{env, io, thread, time::Duration};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::{
    acer_wmi::{
        apply_fan_behavior, apply_fan_speed, clamp_manual_fan_percent, decode_gm_output_byte,
        FAN_BEHAVIOR_AUTO, FAN_BEHAVIOR_CUSTOM_MIXED, FAN_BEHAVIOR_MAX, FAN_SELECTOR_CPU,
        FAN_SELECTOR_GPU, MIN_MANUAL_FAN_PERCENT,
    },
    models::{
        AppliedFanControlSnapshot, ApplyCustomFanCurvesRequest, ApplyFanProfileRequest,
        ControlLayerStatus, ControlSnapshot, FanCurvePoint, FanCurveSet, FanProfileId,
        FanSpeedCalibrationPoint, FanSpeedCalibrationSnapshot, PowerProfileId,
        QuietAutoFanCalibration, QuietAutoFanMap, QuietAutoThermalWarning,
    },
    nvidia_power::{format_power_limit_delta, read_power_readback},
    state,
};

const QUIET_AUTO_IDLE_RPM: u16 = 1600;
const QUIET_AUTO_ELEVATED_RPM: u16 = 2200;
const QUIET_AUTO_ELEVATE_TEMP_C: u8 = 80;
const QUIET_AUTO_IDLE_RESUME_TEMP_C: u8 = 75;
const QUIET_AUTO_WARNING_TEMP_C: u8 = 90;
const QUIET_AUTO_IDLE_MAX_PERCENT: u8 = 25;
const QUIET_AUTO_ELEVATED_MAX_PERCENT: u8 = 35;
const QUIET_AUTO_ELEVATED_DEFAULT_PERCENT: u8 = 8;
const QUIET_AUTO_RPM_LOW_TOLERANCE: u16 = 120;
const QUIET_AUTO_RPM_HIGH_TOLERANCE: u16 = 220;
const FAN_SPEED_CALIBRATION_SETTLE_SECS: u64 = 20;
const FAN_SPEED_CALIBRATION_CANCEL_POLL_MS: u64 = 250;

pub fn apply_fan_profile(
    paths: &ServicePaths,
    request: ApplyFanProfileRequest,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    match request.profile_id {
        FanProfileId::Auto => apply_firmware_wmi_fan_control(
            paths,
            FanProfileId::Auto,
            None,
            None,
            None,
            "Auto fan profile requested through ROOT\\WMI AcerGamingFunction.",
        ),
        FanProfileId::Cool => apply_cool_fan_profile(paths, false),
        FanProfileId::Max => apply_firmware_wmi_fan_control(
            paths,
            FanProfileId::Max,
            None,
            Some(100),
            Some(100),
            "Max fan profile requested through ROOT\\WMI AcerGamingFunction. AeroForge pairs the max behavior write with explicit 100% CPU and GPU fan targets because some Acer firmware only reaches full RPM when both paths are driven together.",
        ),
        FanProfileId::Custom => Err(
            "Custom fan mode requires explicit saved curves instead of a fallback fixed speed."
                .into(),
        ),
    }
}

pub fn apply_cool_fan_profile(
    paths: &ServicePaths,
    quiet_success_log: bool,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    apply_curve_backed_fan_control(
        paths,
        FanProfileId::Cool,
        cool_fan_curve_set(),
        quiet_success_log,
    )
}

pub fn apply_custom_fan_curves(
    paths: &ServicePaths,
    request: ApplyCustomFanCurvesRequest,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    apply_curve_backed_fan_control(
        paths,
        FanProfileId::Custom,
        request.curves,
        request.quiet_success_log,
    )
}

fn apply_curve_backed_fan_control(
    paths: &ServicePaths,
    profile_id: FanProfileId,
    curves: FanCurveSet,
    quiet_success_log: bool,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let targets = resolve_curve_backed_targets(paths, profile_id.clone(), curves);
    apply_curve_backed_targets(paths, targets, quiet_success_log)
}

pub(crate) fn apply_curve_backed_targets(
    paths: &ServicePaths,
    targets: CurveBackedFanTargets,
    quiet_success_log: bool,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let profile_label = curve_backed_profile_label(&targets.profile_id);
    let context = format!(
        "{profile_label} compressed to current-temperature targets: CPU {}C -> requested {}% / applied {}%, GPU {}C -> requested {}% / applied {}%. Zero-percent targets are allowed only at 50C or below; above that AeroForge keeps the {MIN_MANUAL_FAN_PERCENT}% firmware safety floor. AcerGamingFunction accepts direct per-fan target speeds, not a full curve table.",
        targets.cpu_temp_c,
        targets.requested_cpu_speed,
        targets.cpu_speed_percent,
        targets.gpu_temp_c,
        targets.requested_gpu_speed,
        targets.gpu_speed_percent,
    );

    apply_custom_wmi_fan_control(
        paths,
        targets.profile_id,
        targets.curves,
        Some(targets.cpu_speed_percent),
        Some(targets.gpu_speed_percent),
        &context,
        !quiet_success_log,
    )
}

pub(crate) struct CurveBackedFanTargets {
    pub profile_id: FanProfileId,
    pub curves: FanCurveSet,
    pub cpu_speed_percent: u8,
    pub gpu_speed_percent: u8,
    cpu_temp_c: u8,
    gpu_temp_c: u8,
    requested_cpu_speed: u8,
    requested_gpu_speed: u8,
}

pub(crate) fn resolve_curve_backed_targets(
    paths: &ServicePaths,
    profile_id: FanProfileId,
    curves: FanCurveSet,
) -> CurveBackedFanTargets {
    let curves = sanitize_fan_curves(curves);
    let temperatures = read_current_temperatures(paths);
    let cpu_temp_c = temperatures.cpu_temp_c.unwrap_or(65);
    let gpu_temp_c = temperatures.gpu_temp_c.unwrap_or(65);
    let requested_cpu_speed = interpolate_curve_speed(&curves.cpu, cpu_temp_c);
    let requested_gpu_speed = interpolate_curve_speed(&curves.gpu, gpu_temp_c);
    let cpu_speed_percent = clamp_custom_curve_fan_percent(requested_cpu_speed, cpu_temp_c);
    let gpu_speed_percent = clamp_custom_curve_fan_percent(requested_gpu_speed, gpu_temp_c);

    CurveBackedFanTargets {
        profile_id,
        curves,
        cpu_speed_percent,
        gpu_speed_percent,
        cpu_temp_c,
        gpu_temp_c,
        requested_cpu_speed,
        requested_gpu_speed,
    }
}

pub(crate) fn cool_fan_curve_set() -> FanCurveSet {
    let points = vec![
        FanCurvePoint {
            temp_c: 30,
            speed_percent: 20,
        },
        FanCurvePoint {
            temp_c: 50,
            speed_percent: 35,
        },
        FanCurvePoint {
            temp_c: 65,
            speed_percent: 55,
        },
        FanCurvePoint {
            temp_c: 75,
            speed_percent: 78,
        },
        FanCurvePoint {
            temp_c: 82,
            speed_percent: 100,
        },
    ];

    FanCurveSet {
        cpu: points.clone(),
        gpu: points,
    }
}

fn curve_backed_profile_label(profile_id: &FanProfileId) -> &'static str {
    match profile_id {
        FanProfileId::Cool => "Cool fan mode's built-in adaptive curve",
        FanProfileId::Custom => "Custom fan curves",
        FanProfileId::Auto => "Auto fan profile",
        FanProfileId::Max => "Max fan profile",
    }
}

pub fn apply_quiet_auto_fan_policy(
    paths: &ServicePaths,
    previous: &ControlSnapshot,
    log_success: bool,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let now = unix_timestamp();
    let verification_before = build_fan_verification();
    let temperatures = read_current_temperatures(paths);
    let hottest = hottest_temperature(&verification_before, &temperatures);
    let target_rpm =
        quiet_auto_target_rpm(previous.quiet_auto_fan_calibration.last_target_rpm, hottest);
    let target_changed = previous.quiet_auto_fan_calibration.last_target_rpm != Some(target_rpm);

    let mut calibration = previous.quiet_auto_fan_calibration.clone();
    let cpu_speed_percent = next_quiet_auto_percent(
        &mut calibration.cpu,
        target_rpm,
        verification_before.cpu_fan_rpm,
        target_changed,
    );
    let gpu_speed_percent = next_quiet_auto_percent(
        &mut calibration.gpu,
        target_rpm,
        verification_before.gpu_fan_rpm,
        target_changed,
    );
    calibration.last_target_rpm = Some(target_rpm);
    calibration.updated_at_unix = Some(now);

    let warning = quiet_auto_thermal_warning(hottest, now);
    let context = quiet_auto_context(
        target_rpm,
        hottest,
        cpu_speed_percent,
        gpu_speed_percent,
        &calibration,
    );

    if log_success {
        write_log_line(
            &paths.component_log("control-fan"),
            "INFO",
            &format!("Applying Quiet Auto closed-loop fan policy. {context}"),
        )?;
    }

    let strategy = select_custom_fan_strategy(paths);
    let mut behavior_results = Vec::new();
    if let Some(behavior_input) = strategy.behavior_input {
        let result = apply_custom_behavior_latch(paths, &strategy, behavior_input, "before")?;
        if wmi_output_accepted(&result) {
            thread::sleep(Duration::from_millis(150));
        }
        behavior_results.push(result);
    }

    let speed_results =
        match apply_direct_speed_targets(Some(cpu_speed_percent), Some(gpu_speed_percent)) {
            Ok(results) => results,
            Err(error) => {
                let detail = format!(
                    "Quiet Auto fan speed write failed before all targets were accepted: {error}."
                );
                write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
                return Err(io::Error::other(detail).into());
            }
        };

    if let Some(rejected) = speed_results
        .iter()
        .find(|result| !wmi_output_accepted(result))
    {
        let detail = format!(
            "Quiet Auto fan speed target was rejected by AcerGamingFunction {} input {} gmOutput {:?}.",
            rejected.method, rejected.input, rejected.output
        );
        write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
        return Err(io::Error::other(detail).into());
    }

    let mut speed_results = speed_results;
    if let Some(behavior_input) = strategy.behavior_input {
        thread::sleep(Duration::from_millis(150));
        behavior_results.push(apply_custom_behavior_latch(
            paths,
            &strategy,
            behavior_input,
            "after",
        )?);
        thread::sleep(Duration::from_millis(150));
        let reinforcement =
            apply_direct_speed_targets(Some(cpu_speed_percent), Some(gpu_speed_percent))?;
        if let Some(rejected) = reinforcement
            .iter()
            .find(|result| !wmi_output_accepted(result))
        {
            let detail = format!(
                "Quiet Auto fan reinforcement target was rejected by AcerGamingFunction {} input {} gmOutput {:?}.",
                rejected.method, rejected.input, rejected.output
            );
            write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
            return Err(io::Error::other(detail).into());
        }
        speed_results.extend(reinforcement);
    }

    let verification_after = build_fan_verification();
    let readback = Some(json!({
        "backend": "acer-gaming-wmi",
        "namespace": "ROOT\\WMI",
        "class": "AcerGamingFunction",
        "instance": "ACPI\\PNP0C14\\APGe_0",
        "strategy": strategy.id,
        "strategyReason": strategy.reason,
        "quietAuto": {
            "targetRpm": target_rpm,
            "idleRpm": QUIET_AUTO_IDLE_RPM,
            "elevatedRpm": QUIET_AUTO_ELEVATED_RPM,
            "elevateTempC": QUIET_AUTO_ELEVATE_TEMP_C,
            "resumeTempC": QUIET_AUTO_IDLE_RESUME_TEMP_C,
            "warningTempC": QUIET_AUTO_WARNING_TEMP_C,
            "hottest": hottest.map(|(sensor, temp_c)| json!({
                "sensor": sensor,
                "tempC": temp_c,
            })),
            "calibration": calibration.clone(),
            "thermalWarning": warning.clone(),
        },
        "behavior": behavior_results.iter().map(wmi_result_json).collect::<Vec<_>>(),
        "speeds": speed_results.iter().map(wmi_result_json).collect::<Vec<_>>(),
        "verificationBefore": verification_before,
        "verification": verification_after,
    }));

    let detail = build_quiet_auto_apply_detail(
        &context,
        &strategy,
        &behavior_results,
        cpu_speed_percent,
        gpu_speed_percent,
        &verification_after.describe(),
        &warning,
    );
    if log_success {
        write_log_line(&paths.component_log("control-fan"), "INFO", &detail)?;
    }

    let layer_statuses = vec![
        classify_behavior_sequence_status(&behavior_results),
        classify_speed_status(&speed_results),
        classify_fan_verification_status(&verification_after),
    ];

    Ok(AppliedFanControlSnapshot {
        profile_id: FanProfileId::Auto,
        curves: None,
        cpu_speed_percent: Some(cpu_speed_percent),
        gpu_speed_percent: Some(gpu_speed_percent),
        layer_statuses,
        readback,
        quiet_auto_fan_calibration: Some(calibration),
        quiet_auto_thermal_warning: Some(warning),
        applied_at_unix: now,
        detail,
    })
}

pub fn run_fan_speed_calibration(
    paths: &ServicePaths,
    cancel_requested: &dyn Fn() -> bool,
) -> Result<FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let initial_snapshot = state::load_snapshot(paths)?;
    let started_at = unix_timestamp();
    let mut calibration = FanSpeedCalibrationSnapshot {
        running: true,
        status: "Fan speed calibration started. Testing every 5% with a 20 second settle before each RPM readback.".into(),
        started_at_unix: Some(started_at),
        updated_at_unix: Some(started_at),
        completed_at_unix: None,
        current_percent: None,
        settle_seconds: FAN_SPEED_CALIBRATION_SETTLE_SECS,
        points: Vec::new(),
        last_error: None,
    };
    state::persist_fan_speed_calibration(paths, calibration.clone())?;

    let strategy = select_custom_fan_strategy(paths);
    match run_fan_speed_calibration_steps(paths, &strategy, &mut calibration, cancel_requested) {
        Ok(CalibrationStop::Completed) => {}
        Ok(CalibrationStop::Canceled(percent)) => {
            return finish_canceled_calibration(paths, &initial_snapshot, calibration, percent);
        }
        Err(error) => {
            // Never leave the fans latched at a calibration step; restore before surfacing the failure.
            if let Err(restore_error) =
                restore_fan_profile_after_calibration(paths, &initial_snapshot)
            {
                let _ = write_log_line(
                    &paths.component_log("control-fan"),
                    "ERROR",
                    &format!(
                        "Fan profile restore after calibration failure also failed: {restore_error}"
                    ),
                );
            }
            return Err(error);
        }
    }

    let restore_detail = restore_fan_profile_after_calibration(paths, &initial_snapshot)?;
    let completed_at = unix_timestamp();
    calibration.running = false;
    calibration.current_percent = None;
    calibration.updated_at_unix = Some(completed_at);
    calibration.completed_at_unix = Some(completed_at);
    calibration.last_error = None;
    calibration.status = format!(
        "Fan speed calibration complete: recorded {} points. {restore_detail}",
        calibration.points.len()
    );
    state::persist_fan_speed_calibration(paths, calibration.clone())?;
    Ok(calibration)
}

enum CalibrationStop {
    Completed,
    Canceled(Option<u8>),
}

fn run_fan_speed_calibration_steps(
    paths: &ServicePaths,
    strategy: &CustomFanStrategy,
    calibration: &mut FanSpeedCalibrationSnapshot,
    cancel_requested: &dyn Fn() -> bool,
) -> Result<CalibrationStop, Box<dyn std::error::Error + Send + Sync>> {
    if cancel_requested() {
        return Ok(CalibrationStop::Canceled(None));
    }

    for percent in (0u8..=100).step_by(5) {
        if cancel_requested() {
            return Ok(CalibrationStop::Canceled(Some(percent)));
        }

        calibration.current_percent = Some(percent);
        calibration.updated_at_unix = Some(unix_timestamp());
        calibration.status = format!("Testing {percent}% fan speed with firmware latch.");
        state::persist_fan_speed_calibration(paths, calibration.clone())?;

        apply_calibration_speed_target(paths, strategy, percent)?;

        if cancel_requested() || !settle_fan_speed_calibration(cancel_requested) {
            return Ok(CalibrationStop::Canceled(Some(percent)));
        }

        let verification = build_fan_verification();
        calibration.points.retain(|point| point.percent != percent);
        calibration.points.push(FanSpeedCalibrationPoint {
            percent,
            cpu_rpm: verification.cpu_fan_rpm,
            gpu_rpm: verification.gpu_fan_rpm,
            cpu_temp_c: verification.cpu_temp_c,
            gpu_temp_c: verification.gpu_temp_c,
            system_temp_c: verification.system_temp_c,
            sampled_at_unix: unix_timestamp(),
        });
        calibration.updated_at_unix = Some(unix_timestamp());
        calibration.status = format!("Recorded {percent}% fan speed.");
        state::persist_fan_speed_calibration(paths, calibration.clone())?;
    }

    Ok(CalibrationStop::Completed)
}

fn apply_calibration_speed_target(
    paths: &ServicePaths,
    strategy: &CustomFanStrategy,
    percent: u8,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    write_log_line(
        &paths.component_log("control-fan"),
        "INFO",
        &format!(
            "Fan speed calibration applying {percent}% with strategy {}. {}",
            strategy.id, strategy.reason
        ),
    )?;

    let mut behavior_results = Vec::new();
    if let Some(behavior_input) = strategy.behavior_input {
        let result =
            apply_custom_behavior_latch(paths, strategy, behavior_input, "calibration-before")?;
        if wmi_output_accepted(&result) {
            thread::sleep(Duration::from_millis(150));
        }
        behavior_results.push(result);
    }

    let mut speed_results = match apply_direct_speed_targets(Some(percent), Some(percent)) {
        Ok(results) => results,
        Err(error) => {
            let detail = format!(
                "Fan speed calibration {percent}% write failed before all targets were accepted: {error}."
            );
            write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
            return Err(io::Error::other(detail).into());
        }
    };
    reject_failed_calibration_speed_results(percent, "target", &speed_results)?;

    if let Some(behavior_input) = strategy.behavior_input {
        thread::sleep(Duration::from_millis(150));
        behavior_results.push(apply_custom_behavior_latch(
            paths,
            strategy,
            behavior_input,
            "calibration-after",
        )?);
        thread::sleep(Duration::from_millis(150));
        let reinforcement = apply_direct_speed_targets(Some(percent), Some(percent))?;
        reject_failed_calibration_speed_results(percent, "reinforcement", &reinforcement)?;
        speed_results.extend(reinforcement);
    }

    write_log_line(
        &paths.component_log("control-fan"),
        "INFO",
        &format!(
            "Fan speed calibration {percent}% accepted. Behavior [{}]. Speeds [{}].",
            behavior_results
                .iter()
                .map(|result| format!("0x{:08X}->{:?}", result.input, result.output))
                .collect::<Vec<_>>()
                .join(", "),
            speed_results
                .iter()
                .map(|result| format!(
                    "{}:0x{:08X}->{:?}",
                    result.method, result.input, result.output
                ))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    )?;

    Ok(())
}

fn reject_failed_calibration_speed_results(
    percent: u8,
    phase: &str,
    speed_results: &[super::acer_wmi::AcerWmiMethodResult],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(rejected) = speed_results
        .iter()
        .find(|result| !wmi_output_accepted(result))
    {
        return Err(io::Error::other(format!(
            "Fan speed calibration {phase} {percent}% was rejected by AcerGamingFunction {} input {} gmOutput {:?}.",
            rejected.method, rejected.input, rejected.output
        ))
        .into());
    }

    Ok(())
}

fn settle_fan_speed_calibration(cancel_requested: &dyn Fn() -> bool) -> bool {
    let total_ms = FAN_SPEED_CALIBRATION_SETTLE_SECS.saturating_mul(1000);
    let mut waited_ms = 0;
    while waited_ms < total_ms {
        if cancel_requested() {
            return false;
        }

        let remaining_ms = total_ms.saturating_sub(waited_ms);
        let sleep_ms = remaining_ms.min(FAN_SPEED_CALIBRATION_CANCEL_POLL_MS);
        thread::sleep(Duration::from_millis(sleep_ms));
        waited_ms = waited_ms.saturating_add(sleep_ms);
    }

    !cancel_requested()
}

fn finish_canceled_calibration(
    paths: &ServicePaths,
    initial_snapshot: &ControlSnapshot,
    mut calibration: FanSpeedCalibrationSnapshot,
    percent: Option<u8>,
) -> Result<FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let restore_detail = restore_fan_profile_after_calibration(paths, initial_snapshot)?;
    let completed_at = unix_timestamp();
    calibration.running = false;
    calibration.current_percent = None;
    calibration.updated_at_unix = Some(completed_at);
    calibration.completed_at_unix = Some(completed_at);
    calibration.last_error = None;
    calibration.status = match percent {
        Some(percent) => format!(
            "Fan speed calibration canceled at {percent}%: recorded {} points. {restore_detail}",
            calibration.points.len()
        ),
        None => format!(
            "Fan speed calibration canceled before the first target: recorded 0 points. {restore_detail}"
        ),
    };
    state::persist_fan_speed_calibration(paths, calibration.clone())?;
    Ok(calibration)
}

fn restore_fan_profile_after_calibration(
    paths: &ServicePaths,
    previous: &ControlSnapshot,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match previous
        .active_fan_profile
        .clone()
        .unwrap_or(FanProfileId::Auto)
    {
        FanProfileId::Auto => {
            let applied = if matches!(
                previous.active_power_profile,
                Some(PowerProfileId::BatteryGuard)
            ) {
                apply_quiet_auto_fan_policy(paths, previous, false)?
            } else {
                apply_fan_profile(
                    paths,
                    ApplyFanProfileRequest {
                        profile_id: FanProfileId::Auto,
                    },
                )?
            };
            Ok(format!("Restored Auto fan mode. {}", applied.detail))
        }
        FanProfileId::Max => {
            let applied = apply_fan_profile(
                paths,
                ApplyFanProfileRequest {
                    profile_id: FanProfileId::Max,
                },
            )?;
            Ok(format!("Restored Max fan mode. {}", applied.detail))
        }
        FanProfileId::Cool => {
            let applied = apply_cool_fan_profile(paths, false)?;
            Ok(format!("Restored Cool fan mode. {}", applied.detail))
        }
        FanProfileId::Custom => {
            if let Some(curves) = previous.active_fan_curves.clone() {
                let applied = apply_custom_fan_curves(
                    paths,
                    ApplyCustomFanCurvesRequest {
                        curves,
                        quiet_success_log: false,
                    },
                )?;
                Ok(format!("Restored Custom fan curve. {}", applied.detail))
            } else {
                let applied = apply_fan_profile(
                    paths,
                    ApplyFanProfileRequest {
                        profile_id: FanProfileId::Auto,
                    },
                )?;
                Ok(format!(
                    "Previous Custom fan mode had no saved curve; restored Auto. {}",
                    applied.detail
                ))
            }
        }
    }
}

fn clamp_custom_curve_fan_percent(percent: u8, temp_c: u8) -> u8 {
    if percent == 0 && temp_c <= 50 {
        0
    } else if percent == 0 {
        MIN_MANUAL_FAN_PERCENT
    } else {
        clamp_manual_fan_percent(percent)
    }
}

fn apply_firmware_wmi_fan_control(
    paths: &ServicePaths,
    profile_id: FanProfileId,
    curves: Option<FanCurveSet>,
    cpu_speed_percent: Option<u8>,
    gpu_speed_percent: Option<u8>,
    context: &str,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    write_log_line(
        &paths.component_log("control-fan"),
        "INFO",
        &format!("Applying fan profile {}. {}", profile_id.as_str(), context),
    )?;

    let power_before = read_power_readback(paths);
    let behavior_input = behavior_input_for_profile(&profile_id);
    let behavior_result = apply_fan_behavior(behavior_input)?;

    let mut speed_results = Vec::new();
    if let Some(cpu_speed) = cpu_speed_percent {
        speed_results.push(apply_fan_speed(FAN_SELECTOR_CPU, cpu_speed)?);
    }
    if let Some(gpu_speed) = gpu_speed_percent {
        speed_results.push(apply_fan_speed(FAN_SELECTOR_GPU, gpu_speed)?);
    }

    thread::sleep(Duration::from_millis(500));
    let power_after = read_power_readback(paths);
    let power_detail = format_power_limit_delta(&power_before, &power_after);

    let verification = build_fan_verification();
    let readback = Some(json!({
        "backend": "acer-gaming-wmi",
        "namespace": "ROOT\\WMI",
        "class": "AcerGamingFunction",
        "instance": "ACPI\\PNP0C14\\APGe_0",
        "behavior": {
            "method": behavior_result.method,
            "input": behavior_result.input,
            "hresult": format!("0x{:08X}", behavior_result.hresult as u32),
            "gmOutput": behavior_result.output,
            "accepted": wmi_output_accepted(&behavior_result),
        },
        "speeds": speed_results.iter().map(|result| {
            json!({
                "method": result.method,
                "input": result.input,
                "hresult": format!("0x{:08X}", result.hresult as u32),
                "gmOutput": result.output,
                "accepted": wmi_output_accepted(result),
            })
        }).collect::<Vec<_>>(),
        "verification": verification,
        "nvidiaPower": {
            "before": power_before.clone(),
            "after": power_after.clone(),
        },
    }));

    let verification_detail = verification.detail();
    let detail = format!(
        "{} {}",
        build_apply_detail(
            profile_id.as_str(),
            context,
            behavior_input,
            cpu_speed_percent,
            gpu_speed_percent,
            &verification_detail,
        ),
        power_detail
    );
    let layer_statuses = vec![
        classify_wmi_method_status("fan-behavior", "Fan Behavior", &behavior_result),
        classify_speed_status(&speed_results),
        classify_fan_verification_status(&verification),
    ];
    write_log_line(&paths.component_log("control-fan"), "INFO", &detail)?;

    Ok(AppliedFanControlSnapshot {
        profile_id,
        curves,
        cpu_speed_percent,
        gpu_speed_percent,
        layer_statuses,
        readback,
        quiet_auto_fan_calibration: None,
        quiet_auto_thermal_warning: None,
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn apply_custom_wmi_fan_control(
    paths: &ServicePaths,
    profile_id: FanProfileId,
    curves: FanCurveSet,
    cpu_speed_percent: Option<u8>,
    gpu_speed_percent: Option<u8>,
    context: &str,
    log_success: bool,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    if log_success {
        write_log_line(
            &paths.component_log("control-fan"),
            "INFO",
            &format!(
                "Applying {} fan curve by direct speed target. {}",
                profile_id.as_str(),
                context
            ),
        )?;
    }

    let strategy = select_custom_fan_strategy(paths);
    let mut behavior_results = Vec::new();
    if let Some(behavior_input) = strategy.behavior_input {
        let result = apply_custom_behavior_latch(paths, &strategy, behavior_input, "before")?;
        if wmi_output_accepted(&result) {
            thread::sleep(Duration::from_millis(150));
        }
        behavior_results.push(result);
    }

    let speed_results = match apply_direct_speed_targets(cpu_speed_percent, gpu_speed_percent) {
        Ok(results) => results,
        Err(error) => {
            let detail =
                format!("Custom fan speed write failed before all targets were accepted: {error}.");
            write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
            return Err(io::Error::other(detail).into());
        }
    };

    if let Some(rejected) = speed_results
        .iter()
        .find(|result| !wmi_output_accepted(result))
    {
        let detail = format!(
            "Custom fan speed target was rejected by AcerGamingFunction {} input {} gmOutput {:?}.",
            rejected.method, rejected.input, rejected.output
        );
        write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
        return Err(io::Error::other(detail).into());
    }

    let mut speed_results = speed_results;
    if let Some(behavior_input) = strategy.behavior_input {
        thread::sleep(Duration::from_millis(150));
        behavior_results.push(apply_custom_behavior_latch(
            paths,
            &strategy,
            behavior_input,
            "after",
        )?);
        thread::sleep(Duration::from_millis(150));
        let reinforcement = apply_direct_speed_targets(cpu_speed_percent, gpu_speed_percent)?;
        if let Some(rejected) = reinforcement
            .iter()
            .find(|result| !wmi_output_accepted(result))
        {
            let detail = format!(
                "Custom fan reinforcement target was rejected by AcerGamingFunction {} input {} gmOutput {:?}.",
                rejected.method, rejected.input, rejected.output
            );
            write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
            return Err(io::Error::other(detail).into());
        }
        speed_results.extend(reinforcement);
    }

    let verification = build_fan_verification();
    let readback = Some(json!({
        "backend": "acer-gaming-wmi",
        "namespace": "ROOT\\WMI",
        "class": "AcerGamingFunction",
        "instance": "ACPI\\PNP0C14\\APGe_0",
        "strategy": strategy.id,
        "strategyReason": strategy.reason,
        "behavior": behavior_results.iter().map(wmi_result_json).collect::<Vec<_>>(),
        "speeds": speed_results.iter().map(wmi_result_json).collect::<Vec<_>>(),
        "verification": verification,
    }));

    let verification_detail = verification.detail();
    let detail = build_custom_apply_detail(
        &profile_id,
        context,
        &strategy,
        &behavior_results,
        cpu_speed_percent,
        gpu_speed_percent,
        &verification_detail,
    );
    let layer_statuses = vec![
        classify_behavior_sequence_status(&behavior_results),
        classify_speed_status(&speed_results),
        classify_fan_verification_status(&verification),
    ];
    if log_success {
        write_log_line(&paths.component_log("control-fan"), "INFO", &detail)?;
    }

    // Only Custom owns the persisted curve set; Cool must not overwrite the user's saved curves.
    let curves = matches!(profile_id, FanProfileId::Custom).then_some(curves);
    Ok(AppliedFanControlSnapshot {
        profile_id,
        curves,
        cpu_speed_percent,
        gpu_speed_percent,
        layer_statuses,
        readback,
        quiet_auto_fan_calibration: None,
        quiet_auto_thermal_warning: None,
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn apply_custom_behavior_latch(
    paths: &ServicePaths,
    strategy: &CustomFanStrategy,
    behavior_input: u64,
    phase: &str,
) -> Result<super::acer_wmi::AcerWmiMethodResult, Box<dyn std::error::Error + Send + Sync>> {
    let result = apply_fan_behavior(behavior_input)?;
    if !wmi_output_accepted(&result) {
        write_log_line(
            &paths.component_log("control-fan"),
            "WARN",
            &format!(
                "Custom fan strategy {} requested {phase}-speed SetGamingFanBehavior(0x{behavior_input:08X}), but AcerGamingFunction returned gmOutput {:?}. Continuing with direct speed writes. {}",
                strategy.id, result.output, strategy.reason
            ),
        )?;
    }
    Ok(result)
}

fn apply_direct_speed_targets(
    cpu_speed_percent: Option<u8>,
    gpu_speed_percent: Option<u8>,
) -> Result<Vec<super::acer_wmi::AcerWmiMethodResult>, Box<dyn std::error::Error + Send + Sync>> {
    let mut speed_results = Vec::new();
    if let Some(cpu_speed) = cpu_speed_percent {
        speed_results.push(apply_fan_speed(FAN_SELECTOR_CPU, cpu_speed)?);
    }
    if let Some(gpu_speed) = gpu_speed_percent {
        speed_results.push(apply_fan_speed(FAN_SELECTOR_GPU, gpu_speed)?);
    }
    Ok(speed_results)
}

fn behavior_input_for_profile(profile_id: &FanProfileId) -> u64 {
    match profile_id {
        FanProfileId::Auto => FAN_BEHAVIOR_AUTO,
        FanProfileId::Max => FAN_BEHAVIOR_MAX,
        FanProfileId::Cool => FAN_BEHAVIOR_CUSTOM_MIXED,
        FanProfileId::Custom => FAN_BEHAVIOR_CUSTOM_MIXED,
    }
}

fn wmi_output_accepted(result: &super::acer_wmi::AcerWmiMethodResult) -> bool {
    let Some(output) = result.output else {
        return true;
    };
    if matches!(output, 0 | 1) || output == result.input {
        return true;
    }

    let expected = decode_gm_output_byte(result.input);
    expected != 0 && decode_gm_output_byte(output) == expected
}

fn wmi_result_json(result: &super::acer_wmi::AcerWmiMethodResult) -> Value {
    json!({
        "method": result.method,
        "input": result.input,
        "hresult": format!("0x{:08X}", result.hresult as u32),
        "gmOutput": result.output,
        "accepted": wmi_output_accepted(result),
    })
}

fn classify_wmi_method_status(
    id: &'static str,
    label: &'static str,
    result: &super::acer_wmi::AcerWmiMethodResult,
) -> ControlLayerStatus {
    let detail = format!(
        "{} input {} returned gmOutput {:?}.",
        result.method, result.input, result.output
    );
    if wmi_output_accepted(result) {
        ControlLayerStatus::applied(id, label, detail)
    } else {
        ControlLayerStatus::failed(id, label, detail)
    }
}

fn classify_behavior_sequence_status(
    results: &[super::acer_wmi::AcerWmiMethodResult],
) -> ControlLayerStatus {
    if results.is_empty() {
        return ControlLayerStatus::skipped(
            "fan-behavior",
            "Fan Behavior",
            "No SetGamingFanBehavior write was needed for this custom strategy.",
        );
    }
    if let Some(rejected) = results.iter().find(|result| !wmi_output_accepted(result)) {
        return classify_wmi_method_status("fan-behavior", "Fan Behavior", rejected);
    }
    ControlLayerStatus::applied(
        "fan-behavior",
        "Fan Behavior",
        format!(
            "{} SetGamingFanBehavior writes were accepted.",
            results.len()
        ),
    )
}

fn classify_speed_status(results: &[super::acer_wmi::AcerWmiMethodResult]) -> ControlLayerStatus {
    if results.is_empty() {
        return ControlLayerStatus::skipped(
            "fan-speed-targets",
            "Fan Speed Targets",
            "No explicit per-fan speed target was requested.",
        );
    }
    if let Some(rejected) = results.iter().find(|result| !wmi_output_accepted(result)) {
        return classify_wmi_method_status("fan-speed-targets", "Fan Speed Targets", rejected);
    }
    let detail = results
        .iter()
        .map(|result| format!("{} input {}", result.method, result.input))
        .collect::<Vec<_>>()
        .join("; ");
    ControlLayerStatus::applied("fan-speed-targets", "Fan Speed Targets", detail)
}

fn classify_fan_verification_status(verification: &FanVerification) -> ControlLayerStatus {
    let detail = verification.detail();
    if verification.telemetry_available {
        ControlLayerStatus::applied("fan-rpm-readback", "Fan RPM Readback", detail)
    } else {
        ControlLayerStatus::unavailable("fan-rpm-readback", "Fan RPM Readback", detail)
    }
}

fn build_apply_detail(
    profile_id: &str,
    context: &str,
    behavior_input: u64,
    cpu_speed_percent: Option<u8>,
    gpu_speed_percent: Option<u8>,
    verification: &str,
) -> String {
    let speed_detail = match (cpu_speed_percent, gpu_speed_percent) {
        (Some(cpu), Some(gpu)) => {
            format!("CPU speed {cpu}%, GPU speed {gpu}%.")
        }
        _ => "No explicit per-fan speed write was required for this profile.".into(),
    };

    format!(
        "Fan profile {profile_id} was applied through direct AcerGamingFunction WMI/ACPI calls. {context} Behavior input 0x{behavior_input:08X}. {speed_detail} {verification}"
    )
}

fn build_custom_apply_detail(
    profile_id: &FanProfileId,
    context: &str,
    strategy: &CustomFanStrategy,
    behavior_results: &[super::acer_wmi::AcerWmiMethodResult],
    cpu_speed_percent: Option<u8>,
    gpu_speed_percent: Option<u8>,
    verification: &str,
) -> String {
    let speed_detail = match (cpu_speed_percent, gpu_speed_percent) {
        (Some(cpu), Some(gpu)) => {
            format!("CPU speed {cpu}%, GPU speed {gpu}%.")
        }
        _ => "No explicit per-fan speed write was requested.".into(),
    };

    let behavior_detail = if behavior_results.is_empty() {
        format!(
            "No SetGamingFanBehavior write was sent by strategy {}. {}",
            strategy.id, strategy.reason
        )
    } else {
        let outputs = behavior_results
            .iter()
            .map(|result| format!("0x{:08X}->{:?}", result.input, result.output))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "SetGamingFanBehavior was sent before and after direct speeds by strategy {} with outputs [{}]. {}",
            strategy.id, outputs, strategy.reason
        )
    };

    format!(
        "{} fan curve target was applied through direct AcerGamingFunction SetGamingFanSpeed calls. {behavior_detail} {context} {speed_detail} {verification}",
        profile_id.as_str()
    )
}

fn build_quiet_auto_apply_detail(
    context: &str,
    strategy: &CustomFanStrategy,
    behavior_results: &[super::acer_wmi::AcerWmiMethodResult],
    cpu_speed_percent: u8,
    gpu_speed_percent: u8,
    verification: &str,
    warning: &QuietAutoThermalWarning,
) -> String {
    let behavior_detail = if behavior_results.is_empty() {
        format!(
            "No SetGamingFanBehavior write was sent by strategy {}. {}",
            strategy.id, strategy.reason
        )
    } else {
        let outputs = behavior_results
            .iter()
            .map(|result| format!("0x{:08X}->{:?}", result.input, result.output))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "SetGamingFanBehavior was sent before and after direct speeds by strategy {} with outputs [{}]. {}",
            strategy.id, outputs, strategy.reason
        )
    };

    let warning_detail = if warning.active {
        warning
            .message
            .clone()
            .unwrap_or_else(|| "Thermal warning is active.".into())
    } else {
        "No Quiet Auto thermal warning is active.".into()
    };

    format!(
        "Quiet Auto fan policy was applied through direct AcerGamingFunction SetGamingFanSpeed calls. {behavior_detail} {context} CPU speed {cpu_speed_percent}%, GPU speed {gpu_speed_percent}%. {verification} {warning_detail}"
    )
}

fn quiet_auto_target_rpm(
    previous_target_rpm: Option<u16>,
    hottest: Option<(&'static str, u8)>,
) -> u16 {
    match hottest {
        Some((_, temp_c)) if temp_c >= QUIET_AUTO_ELEVATE_TEMP_C => QUIET_AUTO_ELEVATED_RPM,
        Some((_, temp_c)) if temp_c <= QUIET_AUTO_IDLE_RESUME_TEMP_C => QUIET_AUTO_IDLE_RPM,
        Some(_) => previous_target_rpm.unwrap_or(QUIET_AUTO_IDLE_RPM),
        None => QUIET_AUTO_IDLE_RPM,
    }
}

fn next_quiet_auto_percent(
    map: &mut QuietAutoFanMap,
    target_rpm: u16,
    observed_rpm: Option<u16>,
    target_changed: bool,
) -> u8 {
    let stored_percent = if target_rpm == QUIET_AUTO_IDLE_RPM {
        map.idle_percent
    } else {
        map.elevated_percent
    };
    let default_percent = if target_rpm == QUIET_AUTO_IDLE_RPM {
        MIN_MANUAL_FAN_PERCENT
    } else {
        QUIET_AUTO_ELEVATED_DEFAULT_PERCENT
    };

    if let Some(rpm) = observed_rpm {
        map.last_rpm = Some(rpm);
        if let Some(last_percent) = map.last_percent {
            remember_quiet_auto_percent(map, target_rpm, rpm, last_percent);
        }
    }

    let next = if target_changed {
        i16::from(stored_percent.unwrap_or(default_percent))
    } else if let (Some(rpm), Some(last_percent)) = (observed_rpm, map.last_percent) {
        adjust_quiet_auto_percent(last_percent, target_rpm, rpm)
    } else {
        i16::from(
            map.last_percent
                .or(stored_percent)
                .unwrap_or(default_percent),
        )
    };

    let next = clamp_quiet_auto_percent(next, target_rpm);
    map.last_percent = Some(next);
    next
}

fn remember_quiet_auto_percent(
    map: &mut QuietAutoFanMap,
    target_rpm: u16,
    observed_rpm: u16,
    percent: u8,
) {
    let lower = target_rpm.saturating_sub(QUIET_AUTO_RPM_LOW_TOLERANCE);
    let upper = target_rpm.saturating_add(QUIET_AUTO_RPM_HIGH_TOLERANCE);
    if observed_rpm < lower || observed_rpm > upper {
        return;
    }

    if target_rpm == QUIET_AUTO_IDLE_RPM {
        map.idle_percent = Some(percent);
    } else {
        map.elevated_percent = Some(percent);
    }
}

fn adjust_quiet_auto_percent(percent: u8, target_rpm: u16, observed_rpm: u16) -> i16 {
    let delta = i32::from(target_rpm) - i32::from(observed_rpm);
    let mut next = i16::from(percent);

    if delta > 700 {
        next += 6;
    } else if delta > 350 {
        next += 4;
    } else if delta > i32::from(QUIET_AUTO_RPM_LOW_TOLERANCE) {
        next += 2;
    } else if delta < -700 {
        next -= 4;
    } else if delta < -i32::from(QUIET_AUTO_RPM_HIGH_TOLERANCE) {
        next -= 2;
    }

    next
}

fn clamp_quiet_auto_percent(percent: i16, target_rpm: u16) -> u8 {
    let upper = if target_rpm == QUIET_AUTO_IDLE_RPM {
        QUIET_AUTO_IDLE_MAX_PERCENT
    } else {
        QUIET_AUTO_ELEVATED_MAX_PERCENT
    };
    u8::try_from(percent.clamp(i16::from(MIN_MANUAL_FAN_PERCENT), i16::from(upper)))
        .unwrap_or(MIN_MANUAL_FAN_PERCENT)
}

fn quiet_auto_context(
    target_rpm: u16,
    hottest: Option<(&'static str, u8)>,
    cpu_speed_percent: u8,
    gpu_speed_percent: u8,
    calibration: &QuietAutoFanCalibration,
) -> String {
    let thermal_context = hottest
        .map(|(sensor, temp_c)| format!("Hottest sensor {sensor} {temp_c}C."))
        .unwrap_or_else(|| {
            "No current Acer temperature readback was available; using quiet idle target.".into()
        });

    format!(
        "Target {target_rpm} RPM. {thermal_context} CPU request {cpu_speed_percent}% from last {} RPM, GPU request {gpu_speed_percent}% from last {} RPM.",
        display_optional_u16(calibration.cpu.last_rpm),
        display_optional_u16(calibration.gpu.last_rpm),
    )
}

fn quiet_auto_thermal_warning(
    hottest: Option<(&'static str, u8)>,
    now: u64,
) -> QuietAutoThermalWarning {
    let Some((sensor, temp_c)) = hottest else {
        return QuietAutoThermalWarning {
            active: false,
            sensor: None,
            temp_c: None,
            message: None,
            updated_at_unix: Some(now),
        };
    };

    if temp_c < QUIET_AUTO_WARNING_TEMP_C {
        return QuietAutoThermalWarning {
            active: false,
            sensor: Some(sensor.into()),
            temp_c: Some(temp_c),
            message: None,
            updated_at_unix: Some(now),
        };
    }

    let message = format!(
        "{sensor} temperature reached {temp_c}C while Quiet Auto fan control is active. Switch to Max/Turbo or reduce load if it keeps climbing."
    );

    QuietAutoThermalWarning {
        active: true,
        sensor: Some(sensor.into()),
        temp_c: Some(temp_c),
        message: Some(message),
        updated_at_unix: Some(now),
    }
}

fn hottest_temperature(
    verification: &FanVerification,
    temperatures: &CurrentTemperatures,
) -> Option<(&'static str, u8)> {
    let mut hottest: Option<(&'static str, u8)> = None;
    consider_temperature(
        &mut hottest,
        "CPU",
        verification
            .cpu_temp_c
            .and_then(u16_to_u8)
            .or(temperatures.cpu_temp_c),
    );
    consider_temperature(
        &mut hottest,
        "GPU",
        verification
            .gpu_temp_c
            .and_then(u16_to_u8)
            .or(temperatures.gpu_temp_c),
    );
    consider_temperature(
        &mut hottest,
        "System",
        verification.system_temp_c.and_then(u16_to_u8),
    );
    hottest
}

fn consider_temperature(
    current: &mut Option<(&'static str, u8)>,
    sensor: &'static str,
    temp_c: Option<u8>,
) {
    let Some(temp_c) = temp_c else {
        return;
    };

    if current
        .map(|(_, current_temp_c)| temp_c > current_temp_c)
        .unwrap_or(true)
    {
        *current = Some((sensor, temp_c));
    }
}

fn u16_to_u8(value: u16) -> Option<u8> {
    u8::try_from(value).ok()
}

struct CustomFanStrategy {
    id: &'static str,
    reason: String,
    behavior_input: Option<u64>,
}

fn select_custom_fan_strategy(paths: &ServicePaths) -> CustomFanStrategy {
    if let Ok(value) = env::var("AEROFORGE_CUSTOM_FAN_STRATEGY") {
        let normalized = value.trim().to_ascii_lowercase();
        if matches!(
            normalized.as_str(),
            "behavior" | "custom" | "custom-behavior"
        ) {
            return CustomFanStrategy {
                id: "custom-behavior-then-direct-speed",
                reason: "AEROFORGE_CUSTOM_FAN_STRATEGY requested the Acer Custom fan behavior latch before direct speed writes.".into(),
                behavior_input: Some(FAN_BEHAVIOR_CUSTOM_MIXED),
            };
        }
        if matches!(normalized.as_str(), "direct" | "direct-only" | "speed-only") {
            return CustomFanStrategy {
                id: "custom-direct-speed-only",
                reason: "AEROFORGE_CUSTOM_FAN_STRATEGY requested direct speed writes without a firmware behavior latch.".into(),
                behavior_input: None,
            };
        }
    }

    let identity = read_hardware_identity(paths);
    let system_model = identity.system_model.to_ascii_lowercase();
    let cpu_identity = format!("{} {}", identity.cpu_brand, identity.cpu_name).to_ascii_lowercase();

    if system_model.contains("anv15-41") {
        return CustomFanStrategy {
            id: "custom-behavior-then-direct-speed",
            reason: format!(
                "Acer Custom fan behavior latch selected for ANV15-41 so firmware does not keep pulling direct speed targets back to the BIOS fan table. Model '{}', CPU '{}'.",
                identity.system_model, cpu_identity
            ),
            behavior_input: Some(FAN_BEHAVIOR_CUSTOM_MIXED),
        };
    }

    if system_model.contains("anv16-41") {
        return CustomFanStrategy {
            id: "custom-behavior-then-direct-speed",
            reason: format!(
                "Acer Custom fan behavior latch selected for AMD/ANV16-family hardware because direct-only targets can be pulled back by firmware Auto behavior. Model '{}', CPU '{}'.",
                identity.system_model, cpu_identity
            ),
            behavior_input: Some(FAN_BEHAVIOR_CUSTOM_MIXED),
        };
    }

    if system_model.contains("anv15-51")
        || system_model.contains("anv15-52")
        || cpu_identity.contains("genuineintel")
        || cpu_identity.contains("intel")
    {
        return CustomFanStrategy {
            id: "custom-behavior-then-direct-speed",
            reason: format!(
                "Acer Custom fan behavior latch selected for Intel/ANV15-family hardware, including ANV15-51, so firmware stops treating direct speed targets like Auto. Model '{}', CPU '{}'.",
                identity.system_model, cpu_identity
            ),
            behavior_input: Some(FAN_BEHAVIOR_CUSTOM_MIXED),
        };
    }

    if cpu_identity.contains("amd") || cpu_identity.contains("ryzen") {
        return CustomFanStrategy {
            id: "custom-behavior-then-direct-speed",
            reason: format!(
                "Acer Custom fan behavior latch selected for unclassified AMD/Ryzen hardware because Linux Acer WMI implementations use the firmware custom behavior before per-fan speed writes. Model '{}', CPU '{}'.",
                identity.system_model, cpu_identity
            ),
            behavior_input: Some(FAN_BEHAVIOR_CUSTOM_MIXED),
        };
    }

    CustomFanStrategy {
        id: "custom-direct-speed-only",
        reason: format!(
            "Direct-only selected for unknown hardware. Model '{}', CPU '{}'.",
            identity.system_model, cpu_identity
        ),
        behavior_input: None,
    }
}

#[derive(Default)]
struct HardwareIdentity {
    cpu_name: String,
    cpu_brand: String,
    system_model: String,
}

fn read_hardware_identity(paths: &ServicePaths) -> HardwareIdentity {
    let raw = match std::fs::read_to_string(paths.worker_snapshot("telemetry")) {
        Ok(raw) => raw,
        Err(_) => return HardwareIdentity::default(),
    };
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(_) => return HardwareIdentity::default(),
    };

    HardwareIdentity {
        cpu_name: get_string(&value, "cpuName"),
        cpu_brand: get_string(&value, "cpuBrand"),
        system_model: get_string(&value, "systemModel"),
    }
}

fn build_fan_verification() -> FanVerification {
    let mut attempts = Vec::new();
    for attempt in 0..3 {
        let snapshot = super::acer_wmi::read_firmware_sensor_snapshot().ok();
        let verification = FanVerification::from_snapshot(snapshot);
        let complete = verification.telemetry_available
            && (verification.cpu_fan_rpm.unwrap_or(0) > 0
                || verification.gpu_fan_rpm.unwrap_or(0) > 0);
        attempts.push(verification);
        if complete {
            break;
        }
        if attempt < 2 {
            thread::sleep(Duration::from_millis(150));
        }
    }

    attempts.into_iter().last().unwrap_or_default()
}

#[derive(Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FanVerification {
    telemetry_available: bool,
    cpu_fan_rpm: Option<u16>,
    gpu_fan_rpm: Option<u16>,
    cpu_temp_c: Option<u16>,
    gpu_temp_c: Option<u16>,
    system_temp_c: Option<u16>,
}

impl FanVerification {
    fn from_snapshot(snapshot: Option<super::acer_wmi::AcerFirmwareSensorReadback>) -> Self {
        let Some(snapshot) = snapshot else {
            return Self::default();
        };

        Self {
            telemetry_available: snapshot.cpu_fan_rpm.is_some()
                || snapshot.gpu_fan_rpm.is_some()
                || snapshot.cpu_temp_c.is_some()
                || snapshot.gpu_temp_c.is_some()
                || snapshot.system_temp_c.is_some(),
            cpu_fan_rpm: snapshot.cpu_fan_rpm,
            gpu_fan_rpm: snapshot.gpu_fan_rpm,
            cpu_temp_c: snapshot.cpu_temp_c,
            gpu_temp_c: snapshot.gpu_temp_c,
            system_temp_c: snapshot.system_temp_c,
        }
    }

    fn detail(&self) -> String {
        if self.telemetry_available {
            format!("Immediate fan telemetry readback {}.", self.describe())
        } else {
            format!(
                "Immediate fan telemetry verification is unavailable on this machine. {}",
                self.describe()
            )
        }
    }

    fn describe(&self) -> String {
        if !self.telemetry_available {
            return "Acer GetGamingSysInfo did not return CPU/GPU fan or temperature values."
                .into();
        }

        format!(
            "Acer GetGamingSysInfo reported CPU fan {} RPM, GPU fan {} RPM, CPU temp {}, GPU temp {}, system temp {}.",
            display_optional_u16(self.cpu_fan_rpm),
            display_optional_u16(self.gpu_fan_rpm),
            display_optional_u16(self.cpu_temp_c),
            display_optional_u16(self.gpu_temp_c),
            display_optional_u16(self.system_temp_c),
        )
    }
}

fn display_optional_u16(value: Option<u16>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unavailable".into())
}

#[derive(Default)]
struct CurrentTemperatures {
    cpu_temp_c: Option<u8>,
    gpu_temp_c: Option<u8>,
}

fn read_current_temperatures(paths: &ServicePaths) -> CurrentTemperatures {
    let raw = match std::fs::read_to_string(paths.worker_snapshot("telemetry")) {
        Ok(raw) => raw,
        Err(_) => return CurrentTemperatures::default(),
    };
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(_) => return CurrentTemperatures::default(),
    };

    CurrentTemperatures {
        cpu_temp_c: get_u8(&value, &["cpuTempAverageC", "cpuTempC"]),
        gpu_temp_c: get_u8(&value, &["gpuTempC"]),
    }
}

fn get_u8(value: &Value, keys: &[&str]) -> Option<u8> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(value_to_u8))
}

fn get_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn value_to_u8(value: &Value) -> Option<u8> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|value| u8::try_from(value).ok()),
        Value::String(string) => string.trim().parse::<u8>().ok(),
        _ => None,
    }
}

fn sanitize_fan_curves(curves: FanCurveSet) -> FanCurveSet {
    FanCurveSet {
        cpu: sanitize_curve_points(curves.cpu),
        gpu: sanitize_curve_points(curves.gpu),
    }
}

fn sanitize_curve_points(points: Vec<FanCurvePoint>) -> Vec<FanCurvePoint> {
    // The UI edits five points; anything beyond this is not a curve, it is a payload.
    const MAX_CURVE_POINTS: usize = 32;
    let mut sanitized: Vec<FanCurvePoint> = points
        .into_iter()
        .take(MAX_CURVE_POINTS)
        .map(|point| FanCurvePoint {
            temp_c: point.temp_c.clamp(30, 95),
            speed_percent: point.speed_percent.clamp(0, 100),
        })
        .collect();

    if sanitized.is_empty() {
        sanitized = vec![
            FanCurvePoint {
                temp_c: 30,
                speed_percent: 0,
            },
            FanCurvePoint {
                temp_c: 49,
                speed_percent: 0,
            },
            FanCurvePoint {
                temp_c: 65,
                speed_percent: 22,
            },
            FanCurvePoint {
                temp_c: 74,
                speed_percent: 64,
            },
            FanCurvePoint {
                temp_c: 80,
                speed_percent: 100,
            },
        ];
    }

    sanitized.sort_by_key(|point| point.temp_c);
    sanitized.dedup_by_key(|point| point.temp_c);
    let mut speed_floor = 0;
    for point in &mut sanitized {
        point.speed_percent = point.speed_percent.max(speed_floor);
        speed_floor = point.speed_percent;
    }
    sanitized
}

fn interpolate_curve_speed(points: &[FanCurvePoint], temp_c: u8) -> u8 {
    if points.is_empty() {
        return 60;
    }

    if temp_c <= points[0].temp_c {
        return points[0].speed_percent;
    }

    for window in points.windows(2) {
        let lower = &window[0];
        let upper = &window[1];
        if temp_c > upper.temp_c {
            continue;
        }

        let temp_span = u16::from(upper.temp_c.saturating_sub(lower.temp_c)).max(1);
        let temp_offset = u16::from(temp_c.saturating_sub(lower.temp_c));
        let speed_span = i16::from(upper.speed_percent) - i16::from(lower.speed_percent);
        let interpolated = i16::from(lower.speed_percent)
            + (speed_span * i16::try_from(temp_offset).unwrap_or(0))
                / i16::try_from(temp_span).unwrap_or(1);
        return u8::try_from(interpolated.clamp(0, 100)).unwrap_or(60);
    }

    points.last().map(|point| point.speed_percent).unwrap_or(60)
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_custom_curve_fan_percent, classify_fan_verification_status, cool_fan_curve_set,
        interpolate_curve_speed, sanitize_curve_points, FanCurvePoint, FanVerification,
    };
    use crate::workers::control::acer_wmi::MIN_MANUAL_FAN_PERCENT;
    use crate::workers::control::models::ControlLayerState;

    #[test]
    fn fan_verification_status_tracks_telemetry_availability() {
        let missing = FanVerification::default();
        assert_eq!(
            classify_fan_verification_status(&missing).state,
            ControlLayerState::Unavailable
        );

        let present = FanVerification {
            telemetry_available: true,
            cpu_fan_rpm: Some(2400),
            ..FanVerification::default()
        };
        assert_eq!(
            classify_fan_verification_status(&present).state,
            ControlLayerState::Applied
        );
    }

    #[test]
    fn cool_curve_matches_adaptive_targets() {
        let curves = cool_fan_curve_set();
        let cpu_points = curves
            .cpu
            .iter()
            .map(|point| (point.temp_c, point.speed_percent))
            .collect::<Vec<_>>();
        let gpu_points = curves
            .gpu
            .iter()
            .map(|point| (point.temp_c, point.speed_percent))
            .collect::<Vec<_>>();

        let expected = vec![(30, 20), (50, 35), (65, 55), (75, 78), (82, 100)];
        assert_eq!(cpu_points, expected);
        assert_eq!(gpu_points, expected);
    }

    #[test]
    fn cool_curve_interpolates_between_points() {
        let curves = cool_fan_curve_set();

        assert_eq!(interpolate_curve_speed(&curves.cpu, 30), 20);
        assert_eq!(interpolate_curve_speed(&curves.cpu, 57), 44);
        assert_eq!(interpolate_curve_speed(&curves.cpu, 82), 100);
        assert_eq!(interpolate_curve_speed(&curves.cpu, 90), 100);
    }

    #[test]
    fn custom_curve_sanitizer_sorts_clamps_and_keeps_speed_monotonic() {
        let points = sanitize_curve_points(vec![
            super::FanCurvePoint {
                temp_c: 98,
                speed_percent: 10,
            },
            super::FanCurvePoint {
                temp_c: 29,
                speed_percent: 40,
            },
            super::FanCurvePoint {
                temp_c: 55,
                speed_percent: 20,
            },
        ]);

        let tuples = points
            .iter()
            .map(|point| (point.temp_c, point.speed_percent))
            .collect::<Vec<_>>();
        assert_eq!(tuples, vec![(30, 40), (55, 40), (95, 40)]);
    }

    #[test]
    fn manual_zero_speed_gets_safety_floor_above_50c() {
        assert_eq!(clamp_custom_curve_fan_percent(0, 49), 0);
        assert_eq!(
            clamp_custom_curve_fan_percent(0, 65),
            MIN_MANUAL_FAN_PERCENT
        );
    }

    #[test]
    fn oversized_curve_payload_is_truncated_not_persisted() {
        let flood: Vec<FanCurvePoint> = (0..10_000)
            .map(|index| FanCurvePoint {
                temp_c: 30 + (index % 60) as u8,
                speed_percent: 50,
            })
            .collect();
        assert!(sanitize_curve_points(flood).len() <= 32);
    }
}
