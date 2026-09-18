use std::io;

use super::models::AcerHidStatusSnapshot;
use crate::workers::acer_hid_device::{
    open_system_usage_device, SystemUsageDevice, REPORT_ID, REPORT_LEN,
};

const STATUS_GROUP: u8 = 8;

pub fn read_status_snapshot() -> AcerHidStatusSnapshot {
    query_status_snapshot().unwrap_or_default()
}

fn query_status_snapshot() -> Result<AcerHidStatusSnapshot, Box<dyn std::error::Error + Send + Sync>>
{
    let device = open_system_usage_device()?;

    Ok(AcerHidStatusSnapshot {
        cpu_temp_c: query_selector(&device, 1).ok().and_then(to_u8),
        cpu_fan_rpm: query_selector(&device, 2).ok(),
        system_temp_c: query_selector(&device, 3).ok().and_then(to_u8),
        gpu_fan_rpm: query_selector(&device, 6).ok(),
    })
}

fn to_u8(value: u16) -> Option<u8> {
    u8::try_from(value).ok()
}

fn query_selector(device: &SystemUsageDevice, selector: u8) -> io::Result<u16> {
    let mut request = [0u8; REPORT_LEN];
    request[0] = REPORT_ID;
    request[2] = REPORT_ID;
    request[3] = STATUS_GROUP;
    request[5] = 0x02;
    request[6] = selector;

    device.set_feature(&request)?;
    let response = device.get_feature()?;
    Ok(u16::from_le_bytes([response[8], response[9]]))
}
