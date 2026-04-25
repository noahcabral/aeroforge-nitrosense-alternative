use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LowLevelSnapshot {
    pub available: bool,
    pub transport: String,
    pub detail: String,
    pub driver_path: Option<String>,
    pub logical_processor_count: usize,
    pub sampled_processor_count: usize,
    pub tj_max_c: Option<u8>,
    pub package_temp_c: Option<u8>,
    pub average_core_temp_c: Option<u8>,
    pub lowest_core_temp_c: Option<u8>,
    pub highest_core_temp_c: Option<u8>,
    pub core_temps_c: Vec<u8>,
    pub hottest_cores_c: Vec<u8>,
}

impl LowLevelSnapshot {
    pub fn unavailable(detail: String, logical_processor_count: usize) -> Self {
        Self {
            available: false,
            transport: "unavailable".into(),
            detail,
            driver_path: None,
            logical_processor_count,
            sampled_processor_count: 0,
            tj_max_c: None,
            package_temp_c: None,
            average_core_temp_c: None,
            lowest_core_temp_c: None,
            highest_core_temp_c: None,
            core_temps_c: Vec::new(),
            hottest_cores_c: Vec::new(),
        }
    }
}

pub fn hottest_core_average(core_temps: &[u8], hottest_count: usize) -> Option<u8> {
    if core_temps.is_empty() {
        return None;
    }

    let mut sorted = core_temps.to_vec();
    sorted.sort_unstable_by(|left, right| right.cmp(left));

    let sample_len = sorted.len().min(hottest_count.max(1));
    let sample = &sorted[..sample_len];
    Some(
        ((sample.iter().map(|value| *value as u32).sum::<u32>() as f64 / sample.len() as f64)
            .round()
            .clamp(0.0, u8::MAX as f64)) as u8,
    )
}

pub fn hottest_cores(core_temps: &[u8], hottest_count: usize) -> Vec<u8> {
    let mut sorted = core_temps.to_vec();
    sorted.sort_unstable_by(|left, right| right.cmp(left));
    sorted.truncate(sorted.len().min(hottest_count.max(1)));
    sorted
}
