use crate::workers::acer_hid_device::{
    open_system_usage_device, SystemUsageDevice, REPORT_ID, REPORT_LEN,
};
use crate::DynError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemUsageMode {
    Turbo,
    Performance,
    Normal,
    Quiet,
}

impl SystemUsageMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Turbo => "turbo",
            Self::Performance => "performance",
            Self::Normal => "normal",
            Self::Quiet => "quiet",
        }
    }

    fn value(self) -> u8 {
        match self {
            Self::Turbo => 0,
            Self::Performance => 1,
            Self::Normal => 2,
            Self::Quiet => 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AcerHidWriteResult {
    pub action: &'static str,
    pub label: String,
    pub request_prefix: String,
    pub response_prefix: Option<String>,
}

pub fn apply_system_usage_mode(mode: SystemUsageMode) -> Result<AcerHidWriteResult, DynError> {
    let device = open_system_usage_device()?;
    let request = build_system_usage_mode_request(mode);
    send_feature_report(&device, "system-usage-mode", mode.label(), &request, true)
}

pub fn apply_turbo_oc_profile_hint() -> Result<Vec<AcerHidWriteResult>, DynError> {
    let device = open_system_usage_device()?;
    let app_status = build_app_status_request();
    let profile_select = build_oc_profile_select_request(0);
    Ok(vec![
        send_feature_report(&device, "app-status", "enable", &app_status, false)?,
        send_feature_report(
            &device,
            "oc-profile-select",
            "profile-0",
            &profile_select,
            false,
        )?,
    ])
}

fn send_feature_report(
    device: &SystemUsageDevice,
    action: &'static str,
    label: &str,
    request: &[u8; REPORT_LEN],
    read_response: bool,
) -> Result<AcerHidWriteResult, DynError> {
    device.set_feature(request)?;

    let response_prefix = if read_response {
        device
            .get_feature()
            .ok()
            .map(|response| format_hex_prefix(&response, 16))
    } else {
        None
    };

    Ok(AcerHidWriteResult {
        action,
        label: label.into(),
        request_prefix: format_hex_prefix(request, 9),
        response_prefix,
    })
}

fn build_system_usage_mode_request(mode: SystemUsageMode) -> [u8; REPORT_LEN] {
    build_request([
        REPORT_ID,
        0x00,
        REPORT_ID,
        0x01,
        0x00,
        0x01,
        mode.value(),
        0x00,
        0x00,
    ])
}

fn build_app_status_request() -> [u8; REPORT_LEN] {
    build_request([
        REPORT_ID, 0x00, REPORT_ID, 0x03, 0x11, 0x01, 0x03, 0x01, 0x00,
    ])
}

fn build_oc_profile_select_request(profile: u8) -> [u8; REPORT_LEN] {
    build_request([
        REPORT_ID, 0x00, REPORT_ID, 0x02, 0x00, 0x01, profile, 0x00, 0x00,
    ])
}

fn build_request(prefix: [u8; 9]) -> [u8; REPORT_LEN] {
    let mut request = [0u8; REPORT_LEN];
    request[..prefix.len()].copy_from_slice(&prefix);
    request
}

fn format_hex_prefix(bytes: &[u8], count: usize) -> String {
    bytes
        .iter()
        .take(count)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_system_usage_mode_report_prefixes() {
        assert_eq!(
            &build_system_usage_mode_request(SystemUsageMode::Turbo)[..9],
            &[0xA0, 0x00, 0xA0, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            &build_system_usage_mode_request(SystemUsageMode::Quiet)[..9],
            &[0xA0, 0x00, 0xA0, 0x01, 0x00, 0x01, 0x03, 0x00, 0x00]
        );
    }

    #[test]
    fn builds_turbo_oc_hint_prefixes() {
        assert_eq!(
            &build_app_status_request()[..9],
            &[0xA0, 0x00, 0xA0, 0x03, 0x11, 0x01, 0x03, 0x01, 0x00]
        );
        assert_eq!(
            &build_oc_profile_select_request(0)[..9],
            &[0xA0, 0x00, 0xA0, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
    }
}
