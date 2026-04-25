use std::mem::size_of;

use windows_sys::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    System::SystemInformation::GetLogicalProcessorInformationEx,
};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::lowlevel::winring::RELATION_PROCESSOR_CORE,
};

#[repr(C)]
struct LogicalProcessorInfoHeader {
    relationship: u32,
    size: u32,
}

#[repr(C)]
struct GroupAffinity {
    mask: usize,
    group: u16,
    reserved: [u16; 3],
}

#[repr(C)]
struct ProcessorRelationship {
    flags: u8,
    efficiency_class: u8,
    reserved: [u8; 20],
    group_count: u16,
    group_mask: [GroupAffinity; 1],
}

pub fn discover_cpu_topology(paths: &ServicePaths) -> (Vec<usize>, usize) {
    let core_affinity_masks = query_core_affinity_masks().unwrap_or_else(|error| {
        let _ = write_log_line(
            &paths.component_log("lowlevel-topology"),
            "ERROR",
            &format!("Falling back to synthetic affinity masks: {error}"),
        );
        fallback_affinity_masks()
    });

    let logical_processor_count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(core_affinity_masks.len())
        .min(64);

    (core_affinity_masks, logical_processor_count)
}

fn fallback_affinity_masks() -> Vec<usize> {
    let fallback_count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(64);
    (0..fallback_count)
        .filter_map(|index| 1usize.checked_shl(index as u32))
        .collect::<Vec<_>>()
}

fn query_core_affinity_masks() -> Result<Vec<usize>, Box<dyn std::error::Error + Send + Sync>> {
    let mut required_length = 0u32;
    let first_ok = unsafe {
        GetLogicalProcessorInformationEx(
            RELATION_PROCESSOR_CORE,
            std::ptr::null_mut(),
            &mut required_length,
        )
    };

    if first_ok != 0
        || std::io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
    {
        return Err(std::io::Error::last_os_error().into());
    }

    let mut buffer = vec![0u8; required_length as usize];
    let ok = unsafe {
        GetLogicalProcessorInformationEx(
            RELATION_PROCESSOR_CORE,
            buffer.as_mut_ptr().cast(),
            &mut required_length,
        )
    };

    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }

    let mut masks = Vec::new();
    let mut offset = 0usize;
    while offset + size_of::<LogicalProcessorInfoHeader>() <= required_length as usize {
        let header =
            unsafe { &*(buffer.as_ptr().add(offset) as *const LogicalProcessorInfoHeader) };

        if header.size == 0 {
            break;
        }

        if header.relationship == RELATION_PROCESSOR_CORE as u32 {
            let processor = unsafe {
                &*(buffer
                    .as_ptr()
                    .add(offset + size_of::<LogicalProcessorInfoHeader>())
                    as *const ProcessorRelationship)
            };
            let group_masks_ptr = processor.group_mask.as_ptr();
            let group_count = processor.group_count as usize;

            for index in 0..group_count {
                let mask = unsafe { (*group_masks_ptr.add(index)).mask };
                if mask != 0 {
                    masks.push(mask);
                    break;
                }
            }
        }

        offset = offset.saturating_add(header.size as usize);
    }

    if masks.is_empty() {
        return Err("No processor core affinity masks were returned".into());
    }

    Ok(masks)
}
