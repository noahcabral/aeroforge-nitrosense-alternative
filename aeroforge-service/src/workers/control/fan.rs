use serde_json::{json, Value};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

use super::{
    acer_wmi::{
        apply_fan_behavior, apply_fan_speed, clamp_manual_fan_percent, FAN_BEHAVIOR_AUTO,
        FAN_BEHAVIOR_CUSTOM, FAN_BEHAVIOR_MAX, FAN_SELECTOR_CPU, FAN_SELECTOR_GPU,
    },
    models::{
        AppliedFanControlSnapshot, ApplyCustomFanCurvesRequest, ApplyFanProfileRequest,
        FanCurvePoint, FanCurveSet, FanProfileId,
    },
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
            "Max fan profile requested through ROOT\\WMI AcerGamingFunction. On ANV15-52, the max behavior bit alone does not reliably drive the fans to full RPM, so AeroForge also sends explicit 100% CPU and GPU fan targets.",
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

    apply_firmware_wmi_fan_control(
        paths,
        FanProfileId::Custom,
        Some(curves),
        Some(cpu_speed),
        Some(gpu_speed),
        &context,
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

    let behavior_input = behavior_input_for_profile(&profile_id);
    let behavior_result = apply_fan_behavior(behavior_input)?;

    let mut speed_results = Vec::new();
    if let Some(cpu_speed) = cpu_speed_percent {
        speed_results.push(apply_fan_speed(FAN_SELECTOR_CPU, cpu_speed)?);
    }
    if let Some(gpu_speed) = gpu_speed_percent {
        speed_results.push(apply_fan_speed(FAN_SELECTOR_GPU, gpu_speed)?);
    }

    let readback = Some(json!({
        "backend": "acer-gaming-wmi",
        "namespace": "ROOT\\WMI",
        "class": "AcerGamingFunction",
        "instance": "ACPI\\PNP0C14\\APGe_0",
        "behavior": {
            "method": behavior_result.method,
            "input": behavior_result.input,
            "hresult": format!("0x{:08X}", behavior_result.hresult as u32),
        },
        "speeds": speed_results.iter().map(|result| {
            json!({
                "method": result.method,
                "input": result.input,
                "hresult": format!("0x{:08X}", result.hresult as u32),
            })
        }).collect::<Vec<_>>(),
    }));

    let detail = build_apply_detail(
        profile_id.as_str(),
        context,
        behavior_input,
        cpu_speed_percent,
        gpu_speed_percent,
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

fn behavior_input_for_profile(profile_id: &FanProfileId) -> u64 {
    match profile_id {
        FanProfileId::Auto => FAN_BEHAVIOR_AUTO,
        FanProfileId::Max => FAN_BEHAVIOR_MAX,
        FanProfileId::Custom => FAN_BEHAVIOR_CUSTOM,
    }
}

fn build_apply_detail(
    profile_id: &str,
    context: &str,
    behavior_input: u64,
    cpu_speed_percent: Option<u8>,
    gpu_speed_percent: Option<u8>,
) -> String {
    let speed_detail = match (cpu_speed_percent, gpu_speed_percent) {
        (Some(cpu), Some(gpu)) => {
            format!("CPU speed {cpu}%, GPU speed {gpu}%.")
        }
        _ => "No explicit per-fan speed write was required for this profile.".into(),
    };

    format!(
        "Fan profile {profile_id} was applied through direct AcerGamingFunction WMI/ACPI calls. {context} Behavior input 0x{behavior_input:08X}. {speed_detail} RPM movement is verified separately through telemetry."
    )
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
