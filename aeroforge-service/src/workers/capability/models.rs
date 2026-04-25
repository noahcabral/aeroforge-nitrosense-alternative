use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureSupport {
    pub available: bool,
    pub writable: bool,
    pub requires_elevation: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshot {
    pub power_profiles: FeatureSupport,
    pub fan_profiles: FeatureSupport,
    pub fan_curves: FeatureSupport,
    pub smart_charging: FeatureSupport,
    pub usb_power: FeatureSupport,
    pub blue_light_filter: FeatureSupport,
    pub gpu_tuning: FeatureSupport,
    pub boot_logo: FeatureSupport,
    pub notes: Vec<String>,
}

pub fn feature(writable: bool, requires_elevation: bool) -> FeatureSupport {
    FeatureSupport {
        available: true,
        writable,
        requires_elevation,
    }
}
