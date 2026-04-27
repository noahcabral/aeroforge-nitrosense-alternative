use std::{
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    let output = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-Command",
            "$cores = Get-CimInstance Win32_PerfFormattedData_Counters_ProcessorInformation -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^\\d+,\\d+$' }; if ($cores) { $effective = $cores | ForEach-Object { ([double]$_.ProcessorFrequency) * (([double]$_.PercentProcessorPerformance) / 100.0) } | Measure-Object -Average; [int][math]::Round($effective.Average) } else { $total = Get-CimInstance Win32_PerfFormattedData_Counters_ProcessorInformation -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq '_Total' } | Select-Object -First 1; if ($total) { [int][math]::Round(([double]$total.ProcessorFrequency) * (([double]$total.PercentProcessorPerformance) / 100.0)) } }",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
}
