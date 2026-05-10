use crate::{paths::ServicePaths, workers::lowlevel::winring};

use super::models::{CustomPowerBaseId, PowerProfileId};

const QUIET_PL1_W: f32 = 28.0;
const BALANCED_PL1_W: f32 = 45.0;
const PERFORMANCE_PL1_W: f32 = 75.0;

pub(crate) fn apply_profile_package_limit(
    paths: &ServicePaths,
    profile_id: &PowerProfileId,
    custom_base_profile: Option<&CustomPowerBaseId>,
) -> String {
    match apply_profile_package_limit_inner(paths, profile_id, custom_base_profile) {
        Ok(detail) => detail,
        Err(error) => {
            format!("CPU package power-limit write skipped: {error}.")
        }
    }
}

fn apply_profile_package_limit_inner(
    paths: &ServicePaths,
    profile_id: &PowerProfileId,
    custom_base_profile: Option<&CustomPowerBaseId>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let context = winring::WinRingContext::load(paths)?;
    let Some(readback) = context.read_rapl_readback()? else {
        return Ok("CPU RAPL readback unavailable; package power limits were not changed.".into());
    };
    let Some(current) = readback.package_power_limit else {
        return Ok(
            "CPU package power-limit MSR unavailable; package power limits were not changed."
                .into(),
        );
    };
    if current.locked {
        return Ok(format!(
            "CPU package power-limit MSR is locked by firmware; PL1 {} and PL2 {} were left unchanged.",
            format_limit(current.pl1_w),
            format_limit(current.pl2_w)
        ));
    }

    let Some(current_pl2_w) = current.pl2_w else {
        return Ok("CPU PL2 readback unavailable; PL1 target could not be derived safely.".into());
    };

    let Some(target_pl1_w) = target_pl1_w(profile_id, custom_base_profile, current_pl2_w) else {
        return Ok("No CPU package power-limit target is defined for this profile.".into());
    };

    let target_pl1_w = target_pl1_w.min(current_pl2_w);
    let result = context.apply_package_power_limit(winring::PackagePowerLimitWrite {
        pl1_w: Some(target_pl1_w),
        pl2_w: None,
    })?;

    let Some(result) = result else {
        return Ok("CPU package power-limit write returned no readback.".into());
    };

    Ok(format_apply_result(
        profile_id,
        custom_base_profile,
        &result,
    ))
}

fn target_pl1_w(
    profile_id: &PowerProfileId,
    custom_base_profile: Option<&CustomPowerBaseId>,
    current_pl2_w: f32,
) -> Option<f32> {
    let target = match profile_id {
        PowerProfileId::BatteryGuard => QUIET_PL1_W,
        PowerProfileId::Balanced => BALANCED_PL1_W,
        PowerProfileId::Performance => PERFORMANCE_PL1_W,
        PowerProfileId::Turbo => current_pl2_w,
        PowerProfileId::Custom => {
            match custom_base_profile.unwrap_or(&CustomPowerBaseId::Performance) {
                CustomPowerBaseId::Balanced => BALANCED_PL1_W,
                CustomPowerBaseId::Performance => PERFORMANCE_PL1_W,
                CustomPowerBaseId::Turbo => current_pl2_w,
            }
        }
    };

    target.is_finite().then_some(target)
}

fn format_apply_result(
    profile_id: &PowerProfileId,
    custom_base_profile: Option<&CustomPowerBaseId>,
    result: &winring::PackagePowerLimitApplyResult,
) -> String {
    let profile = profile_label(profile_id, custom_base_profile);
    let verb = if result.changed {
        "applied"
    } else {
        "already matched"
    };
    format!(
        "CPU package power-limit {verb} for {profile}: PL1 {} -> {}, PL2 preserved at {}. Raw 0x{:016X} -> 0x{:016X} (readback 0x{:016X}, unit {:.3}W).",
        format_limit(result.before.pl1_w),
        format_limit(result.after.pl1_w),
        format_limit(result.after.pl2_w),
        result.before_raw,
        result.target_raw,
        result.after_raw,
        result.power_unit_w,
    )
}

fn profile_label(
    profile_id: &PowerProfileId,
    custom_base_profile: Option<&CustomPowerBaseId>,
) -> String {
    match profile_id {
        PowerProfileId::BatteryGuard => "quiet".into(),
        PowerProfileId::Balanced => "balanced".into(),
        PowerProfileId::Performance => "performance".into(),
        PowerProfileId::Turbo => "turbo".into(),
        PowerProfileId::Custom => format!(
            "custom/{} base",
            match custom_base_profile.unwrap_or(&CustomPowerBaseId::Performance) {
                CustomPowerBaseId::Balanced => "balanced",
                CustomPowerBaseId::Performance => "performance",
                CustomPowerBaseId::Turbo => "turbo",
            }
        ),
    }
}

fn format_limit(limit: Option<f32>) -> String {
    limit
        .map(|watts| format!("{watts:.1}W"))
        .unwrap_or_else(|| "unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turbo_targets_current_pl2() {
        assert_eq!(
            target_pl1_w(&PowerProfileId::Turbo, None, 115.0),
            Some(115.0)
        );
    }

    #[test]
    fn custom_base_selects_limit_class() {
        assert_eq!(
            target_pl1_w(
                &PowerProfileId::Custom,
                Some(&CustomPowerBaseId::Balanced),
                115.0
            ),
            Some(BALANCED_PL1_W)
        );
        assert_eq!(
            target_pl1_w(
                &PowerProfileId::Custom,
                Some(&CustomPowerBaseId::Turbo),
                115.0
            ),
            Some(115.0)
        );
    }
}
