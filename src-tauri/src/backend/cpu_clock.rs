use std::{
    ffi::c_void,
    mem::size_of,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const PROCESSOR_INFORMATION_LEVEL: u32 = 11;
const STATUS_SUCCESS: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessorPowerInformation {
    number: u32,
    max_mhz: u32,
    current_mhz: u32,
    mhz_limit: u32,
    max_idle_state: u32,
    current_idle_state: u32,
}

extern "system" {
    fn CallNtPowerInformation(
        information_level: u32,
        input_buffer: *mut c_void,
        input_buffer_length: u32,
        output_buffer: *mut c_void,
        output_buffer_length: u32,
    ) -> i32;
}

struct CpuClockCache {
    last_refresh: Option<Instant>,
    value_mhz: u16,
}

static CPU_CLOCK_CACHE: OnceLock<Mutex<CpuClockCache>> = OnceLock::new();
const CPU_CLOCK_REFRESH_INTERVAL: Duration = Duration::from_millis(750);

pub fn read_effective_cpu_clock_mhz() -> Option<u16> {
    let cache = CPU_CLOCK_CACHE.get_or_init(|| {
        Mutex::new(CpuClockCache {
            last_refresh: None,
            value_mhz: 0,
        })
    });

    let mut guard = cache.lock().ok()?;
    let now = Instant::now();
    let should_refresh = guard
        .last_refresh
        .map(|last_refresh| now.duration_since(last_refresh) >= CPU_CLOCK_REFRESH_INTERVAL)
        .unwrap_or(true);

    if should_refresh {
        if let Some(value_mhz) = query_effective_cpu_clock_mhz() {
            guard.value_mhz = value_mhz;
            guard.last_refresh = Some(now);
        } else if guard.last_refresh.is_none() {
            return None;
        }
    }

    (guard.value_mhz > 0).then_some(guard.value_mhz)
}

fn query_effective_cpu_clock_mhz() -> Option<u16> {
    let processor_count = std::thread::available_parallelism().ok()?.get();
    let mut processors = vec![ProcessorPowerInformation::default(); processor_count];
    let status = unsafe {
        CallNtPowerInformation(
            PROCESSOR_INFORMATION_LEVEL,
            std::ptr::null_mut(),
            0,
            processors.as_mut_ptr().cast(),
            (processors.len() * size_of::<ProcessorPowerInformation>()) as u32,
        )
    };

    if status != STATUS_SUCCESS {
        return None;
    }

    let mut total_mhz = 0u64;
    let mut sample_count = 0u64;
    for processor in processors {
        if processor.current_mhz > 0 {
            total_mhz += processor.current_mhz as u64;
            sample_count += 1;
        }
    }

    (sample_count > 0).then_some((total_mhz / sample_count).min(u16::MAX as u64) as u16)
}
