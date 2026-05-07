use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NvidiaPowerReadback {
    pub power_draw_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub default_limit_w: Option<f32>,
    pub min_limit_w: Option<f32>,
    pub max_limit_w: Option<f32>,
    pub source: &'static str,
    pub error: Option<String>,
}

pub(crate) fn read_power_readback() -> NvidiaPowerReadback {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=power.draw,enforced.power.limit,power.default_limit,power.min_limit,power.max_limit",
            "--format=csv,noheader,nounits",
        ])
        .output();

    let Ok(output) = output else {
        return NvidiaPowerReadback {
            source: "nvidia-smi",
            error: Some("nvidia-smi could not be launched".into()),
            ..Default::default()
        };
    };

    if !output.status.success() {
        return NvidiaPowerReadback {
            source: "nvidia-smi",
            error: Some(command_error_detail(&output)),
            ..Default::default()
        };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout.lines().find(|line| !line.trim().is_empty()) else {
        return NvidiaPowerReadback {
            source: "nvidia-smi",
            error: Some("nvidia-smi returned no GPU power row".into()),
            ..Default::default()
        };
    };

    let columns = line.split(',').map(str::trim).collect::<Vec<_>>();

    NvidiaPowerReadback {
        power_draw_w: parse_watts(columns.get(0).copied()),
        power_limit_w: parse_watts(columns.get(1).copied()),
        default_limit_w: parse_watts(columns.get(2).copied()),
        min_limit_w: parse_watts(columns.get(3).copied()),
        max_limit_w: parse_watts(columns.get(4).copied()),
        source: "nvidia-smi",
        error: None,
    }
}

pub(crate) fn format_power_limit_delta(
    before: &NvidiaPowerReadback,
    after: &NvidiaPowerReadback,
) -> String {
    if let Some(error) = after.error.as_deref().or(before.error.as_deref()) {
        return format!("NVIDIA enforced power-limit readback unavailable: {error}.");
    }

    match (before.power_limit_w, after.power_limit_w) {
        (Some(before_limit), Some(after_limit)) => {
            let draw_detail = after
                .power_draw_w
                .map(|draw| format!(" Current draw {:.1}W.", draw))
                .unwrap_or_default();
            let max_detail = after
                .max_limit_w
                .map(|max| format!(" Driver max {:.0}W.", max))
                .unwrap_or_default();
            format!(
                "NVIDIA enforced power limit {:.1}W -> {:.1}W.{}{}",
                before_limit, after_limit, draw_detail, max_detail
            )
        }
        (None, Some(after_limit)) => {
            format!("NVIDIA enforced power limit now reads {:.1}W.", after_limit)
        }
        (Some(before_limit), None) => {
            format!(
                "NVIDIA enforced power limit previously read {:.1}W, but no post-apply limit was available.",
                before_limit
            )
        }
        (None, None) => "NVIDIA enforced power-limit readback returned no usable value.".into(),
    }
}

fn parse_watts(value: Option<&str>) -> Option<f32> {
    let value = value?.trim();
    if value.is_empty()
        || value.eq_ignore_ascii_case("N/A")
        || value.eq_ignore_ascii_case("[N/A]")
        || value.eq_ignore_ascii_case("Not Supported")
    {
        return None;
    }

    value
        .trim_end_matches('W')
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

fn command_error_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }

    format!("nvidia-smi exited with status {}", output.status)
}
