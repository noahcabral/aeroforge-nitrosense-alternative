use serde_json::{json, Value};
use std::{env, thread, time::Duration};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::{
    acer_wmi::{
        apply_fan_behavior, apply_fan_speed, clamp_manual_fan_percent, decode_gm_output_byte,
        FAN_BEHAVIOR_AUTO, FAN_BEHAVIOR_CUSTOM_MIXED, FAN_BEHAVIOR_MAX, FAN_SELECTOR_CPU,
        FAN_SELECTOR_GPU,
    },
    models::{
        AppliedFanControlSnapshot, ApplyCustomFanCurvesRequest, ApplyFanProfileRequest,
        FanCurvePoint, FanCurveSet, FanProfileId,
    },
    nvidia_power::{format_power_limit_delta, read_power_readback},
};

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

pub fn apply_custom_fan_curves(
    paths: &ServicePaths,
    request: ApplyCustomFanCurvesRequest,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let curves = sanitize_fan_curves(request.curves);
    let temperatures = read_current_temperatures(paths);
    let cpu_temp = temperatures.cpu_temp_c.unwrap_or(65);
    let gpu_temp = temperatures.gpu_temp_c.unwrap_or(65);
    let requested_cpu_speed = interpolate_curve_speed(&curves.cpu, cpu_temp);
    let requested_gpu_speed = interpolate_curve_speed(&curves.gpu, gpu_temp);
    let cpu_speed = clamp_manual_fan_percent(requested_cpu_speed);
    let gpu_speed = clamp_manual_fan_percent(requested_gpu_speed);

    let context = format!(
        "Custom fan curves compressed to current-temperature targets: CPU {}C -> requested {}% / applied {}%, GPU {}C -> requested {}% / applied {}%. AcerGamingFunction accepts direct per-fan target speeds, not a full curve table.",
        cpu_temp, requested_cpu_speed, cpu_speed, gpu_temp, requested_gpu_speed, gpu_speed
    );

    apply_custom_wmi_fan_control(
        paths,
        curves,
        Some(cpu_speed),
        Some(gpu_speed),
        &context,
        !request.quiet_success_log,
    )
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
        "verification": build_fan_verification(),
        "nvidiaPower": {
            "before": power_before.clone(),
            "after": power_after.clone(),
        },
    }));

    let verification = build_fan_verification_detail();
    let detail = format!(
        "{} {}",
        build_apply_detail(
            profile_id.as_str(),
            context,
            behavior_input,
            cpu_speed_percent,
            gpu_speed_percent,
            &verification,
        ),
        power_detail
    );
    write_log_line(&paths.component_log("control-fan"), "INFO", &detail)?;

    Ok(AppliedFanControlSnapshot {
        profile_id,
        curves,
        cpu_speed_percent,
        gpu_speed_percent,
        readback,
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn apply_custom_wmi_fan_control(
    paths: &ServicePaths,
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
                "Applying custom fan curve by direct speed target. {}",
                context
            ),
        )?;
    }

    let strategy = select_custom_fan_strategy(paths);
    let behavior_result = if let Some(behavior_input) = strategy.behavior_input {
        let result = apply_fan_behavior(behavior_input)?;
        if wmi_output_accepted(&result) {
            thread::sleep(Duration::from_millis(200));
        } else {
            write_log_line(
                &paths.component_log("control-fan"),
                "WARN",
                &format!(
                    "Custom fan strategy {} requested SetGamingFanBehavior(0x{behavior_input:08X}), but AcerGamingFunction returned gmOutput {:?}. Continuing with direct speed writes. {}",
                    strategy.id, result.output, strategy.reason
                ),
            )?;
        }
        Some(result)
    } else {
        None
    };

    let speed_results = match apply_direct_speed_targets(cpu_speed_percent, gpu_speed_percent) {
        Ok(results) => results,
        Err(error) => {
            let rollback = apply_fan_behavior(FAN_BEHAVIOR_AUTO)?;
            let detail = format!(
                "Custom fan speed write failed before all targets were accepted: {error}. Restored Auto fan behavior with SetGamingFanBehavior({}) and gmOutput {:?}.",
                rollback.input, rollback.output
            );
            write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
            return Ok(AppliedFanControlSnapshot {
                profile_id: FanProfileId::Auto,
                curves: Some(curves),
                cpu_speed_percent: None,
                gpu_speed_percent: None,
                readback: Some(json!({
                    "backend": "acer-gaming-wmi",
                    "strategy": "custom-direct-speed-rejected-restored-auto",
                    "error": error.to_string(),
                    "rollbackBehavior": wmi_result_json(&rollback),
                })),
                applied_at_unix: unix_timestamp(),
                detail,
            });
        }
    };

    if let Some(rejected) = speed_results
        .iter()
        .find(|result| !wmi_output_accepted(result))
    {
        let rollback = apply_fan_behavior(FAN_BEHAVIOR_AUTO)?;
        let detail = format!(
            "Custom fan speed target was rejected by AcerGamingFunction {} input {} gmOutput {:?}. Restored Auto fan behavior with SetGamingFanBehavior({}) and gmOutput {:?}.",
            rejected.method, rejected.input, rejected.output, rollback.input, rollback.output
        );
        write_log_line(&paths.component_log("control-fan"), "WARN", &detail)?;
        return Ok(AppliedFanControlSnapshot {
            profile_id: FanProfileId::Auto,
            curves: Some(curves),
            cpu_speed_percent: None,
            gpu_speed_percent: None,
            readback: Some(json!({
                "backend": "acer-gaming-wmi",
                "strategy": "custom-direct-speed-rejected-restored-auto",
                "rejectedSpeed": wmi_result_json(rejected),
                "rollbackBehavior": wmi_result_json(&rollback),
            })),
            applied_at_unix: unix_timestamp(),
            detail,
        });
    }

    let readback = Some(json!({
        "backend": "acer-gaming-wmi",
        "namespace": "ROOT\\WMI",
        "class": "AcerGamingFunction",
        "instance": "ACPI\\PNP0C14\\APGe_0",
        "strategy": strategy.id,
        "strategyReason": strategy.reason,
        "behavior": behavior_result.as_ref().map(wmi_result_json),
        "speeds": speed_results.iter().map(wmi_result_json).collect::<Vec<_>>(),
        "verification": build_fan_verification(),
    }));

    let verification = build_fan_verification_detail();
    let detail = build_custom_apply_detail(
        context,
        &strategy,
        behavior_result.as_ref(),
        cpu_speed_percent,
        gpu_speed_percent,
        &verification,
    );
    if log_success {
        write_log_line(&paths.component_log("control-fan"), "INFO", &detail)?;
    }

    Ok(AppliedFanControlSnapshot {
        profile_id: FanProfileId::Custom,
        curves: Some(curves),
        cpu_speed_percent,
        gpu_speed_percent,
        readback,
        applied_at_unix: unix_timestamp(),
        detail,
    })
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
    context: &str,
    strategy: &CustomFanStrategy,
    behavior_result: Option<&super::acer_wmi::AcerWmiMethodResult>,
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

    let behavior_detail = match behavior_result {
        Some(result) => format!(
            "SetGamingFanBehavior(0x{:08X}) was sent first by strategy {} and returned gmOutput {:?}. {}",
            result.input, strategy.id, result.output, strategy.reason
        ),
        None => format!(
            "No SetGamingFanBehavior write was sent by strategy {}. {}",
            strategy.id, strategy.reason
        ),
    };

    format!(
        "Custom fan curve target was applied through direct AcerGamingFunction SetGamingFanSpeed calls. {behavior_detail} {context} {speed_detail} {verification}"
    )
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
            id: "custom-direct-speed-only",
            reason: format!(
                "Direct-only selected for AMD/ANV16-family safety. Model '{}', CPU '{}'.",
                identity.system_model, cpu_identity
            ),
            behavior_input: None,
        };
    }

    if system_model.contains("anv15-52")
        || cpu_identity.contains("genuineintel")
        || cpu_identity.contains("intel")
    {
        return CustomFanStrategy {
            id: "custom-behavior-then-direct-speed",
            reason: format!(
                "Acer Custom fan behavior latch selected for Intel/ANV15-family hardware so firmware stops treating direct speed targets like Auto. Model '{}', CPU '{}'.",
                identity.system_model, cpu_identity
            ),
            behavior_input: Some(FAN_BEHAVIOR_CUSTOM_MIXED),
        };
    }

    if cpu_identity.contains("amd") || cpu_identity.contains("ryzen") {
        return CustomFanStrategy {
            id: "custom-direct-speed-only",
            reason: format!(
                "Direct-only selected for unclassified AMD/Ryzen hardware. Model '{}', CPU '{}'.",
                identity.system_model, cpu_identity
            ),
            behavior_input: None,
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

fn build_fan_verification_detail() -> String {
    let verification = build_fan_verification();
    if verification.telemetry_available {
        format!(
            "Immediate fan telemetry readback {}.",
            verification.describe()
        )
    } else {
        format!(
            "Immediate fan telemetry verification is unavailable on this machine. {}",
            verification.describe()
        )
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
    let mut sanitized: Vec<FanCurvePoint> = points
        .into_iter()
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
