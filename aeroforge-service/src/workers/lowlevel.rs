mod models;
mod topology;
pub(crate) mod winring;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use models::LowLevelSnapshot;
use winring::RaplReadback;

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::{
        sleep_until_next_tick, unix_timestamp, WorkerEvent, WorkerEventSender, WorkerRegistration,
        WorkerState,
    },
};

const WORKER_NAME: &str = "lowlevel-worker";
const SAMPLE_INTERVAL: Duration = Duration::from_millis(333);
const LOAD_RETRY_INTERVAL: Duration = Duration::from_secs(15);

pub fn registration() -> WorkerRegistration {
    WorkerRegistration::new(WORKER_NAME, run)
}

fn run(
    paths: ServicePaths,
    stop_flag: Arc<AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut sampler = LowLevelSampler::new();

    while !stop_flag.load(Ordering::SeqCst) {
        let snapshot = sampler.sample(&paths)?;
        std::fs::write(
            paths.worker_snapshot("lowlevel"),
            serde_json::to_string_pretty(&snapshot)?,
        )?;

        let _ = event_tx.send(WorkerEvent {
            worker: WORKER_NAME,
            state: WorkerState::Running,
            message: Some(snapshot.detail.clone()),
            interval_seconds: SAMPLE_INTERVAL.as_secs(),
            timestamp_unix: unix_timestamp(),
        });

        sleep_until_next_tick(SAMPLE_INTERVAL, &stop_flag);
    }

    Ok(())
}

struct LowLevelSampler {
    context: Option<winring::WinRingContext>,
    last_load_attempt: Option<Instant>,
    last_load_error: Option<String>,
    last_rapl_energy: Option<RaplEnergySample>,
}

impl LowLevelSampler {
    fn new() -> Self {
        Self {
            context: None,
            last_load_attempt: None,
            last_load_error: None,
            last_rapl_energy: None,
        }
    }

    fn sample(
        &mut self,
        paths: &ServicePaths,
    ) -> Result<LowLevelSnapshot, Box<dyn std::error::Error + Send + Sync>> {
        self.try_load_context(paths);

        let (core_affinity_masks, logical_processor_count) = topology::discover_cpu_topology(paths);

        let Some(context) = self.context.as_ref() else {
            return Ok(LowLevelSnapshot::unavailable(
                self.last_load_error
                    .clone()
                    .unwrap_or_else(|| "WinRing0 driver is not initialized.".into()),
                logical_processor_count,
            ));
        };

        let tj_max_c = context.read_tj_max().ok().flatten();
        let package_temp_c = context.read_package_temp(tj_max_c).ok().flatten();
        let rapl_readback = context.read_rapl_readback().ok().flatten();

        let mut core_temps = Vec::with_capacity(core_affinity_masks.len());
        for mask in core_affinity_masks.iter().copied() {
            if let Ok(Some(temp_c)) = context.read_core_temp(mask, tj_max_c) {
                core_temps.push(temp_c);
            }
        }

        let driver_path = context.driver_path.display().to_string();
        let package_power_w = self.calculate_package_power_w(rapl_readback);
        let power_limit = rapl_readback.and_then(|readback| readback.package_power_limit);
        let sampled_processor_count = core_temps.len();
        let average_core_temp_c = models::hottest_core_average(&core_temps, 3);
        let lowest_core_temp_c = core_temps.iter().copied().min();
        let highest_core_temp_c = core_temps.iter().copied().max();
        let hottest_cores_c = models::hottest_cores(&core_temps, 3);

        Ok(LowLevelSnapshot {
            available: tj_max_c.is_some()
                || package_temp_c.is_some()
                || average_core_temp_c.is_some(),
            transport: "winring0".into(),
            detail: format!(
                "WinRing0 kernel driver active from {}. Sampled {} physical cores across {} logical processors.",
                driver_path,
                sampled_processor_count,
                logical_processor_count
            ),
            driver_path: Some(driver_path),
            logical_processor_count,
            sampled_processor_count,
            tj_max_c,
            package_temp_c,
            average_core_temp_c,
            lowest_core_temp_c,
            highest_core_temp_c,
            core_temps_c: core_temps,
            hottest_cores_c,
            package_power_w,
            package_power_energy_raw: rapl_readback.map(|readback| readback.package_energy_raw),
            package_power_unit_w: rapl_readback.map(|readback| readback.power_unit_w as f32),
            package_energy_unit_j: rapl_readback.map(|readback| readback.energy_unit_j),
            package_pl1_w: power_limit.and_then(|limit| limit.pl1_w),
            package_pl1_enabled: power_limit.map(|limit| limit.pl1_enabled),
            package_pl2_w: power_limit.and_then(|limit| limit.pl2_w),
            package_pl2_enabled: power_limit.map(|limit| limit.pl2_enabled),
            package_power_limit_locked: power_limit.map(|limit| limit.locked),
        })
    }

    fn calculate_package_power_w(&mut self, readback: Option<RaplReadback>) -> Option<f32> {
        let readback = readback?;
        let current = RaplEnergySample {
            raw_counter: readback.package_energy_raw,
            energy_unit_j: readback.energy_unit_j,
            sampled_at: Instant::now(),
        };
        let previous = self.last_rapl_energy.replace(current)?;
        if (previous.energy_unit_j - current.energy_unit_j).abs() > f64::EPSILON {
            return None;
        }

        let elapsed = current.sampled_at.duration_since(previous.sampled_at);
        let elapsed_seconds = elapsed.as_secs_f64();
        if elapsed_seconds <= 0.0 {
            return None;
        }

        let delta_raw = current.raw_counter.wrapping_sub(previous.raw_counter) as f64;
        let watts = (delta_raw * current.energy_unit_j) / elapsed_seconds;
        if watts.is_finite() && (0.0..=1000.0).contains(&watts) {
            Some(watts as f32)
        } else {
            None
        }
    }

    fn try_load_context(&mut self, paths: &ServicePaths) {
        if self.context.is_some() || !self.should_retry_load() {
            return;
        }

        self.last_load_attempt = Some(Instant::now());
        match winring::WinRingContext::load(paths) {
            Ok(context) => {
                if self.last_load_error.is_some() {
                    let _ = write_log_line(
                        &paths.component_log("lowlevel-init"),
                        "INFO",
                        "Recovered after prior WinRing0 initialization failure.",
                    );
                } else {
                    let _ = write_log_line(
                        &paths.component_log("lowlevel-init"),
                        "INFO",
                        "WinRing0 initialization succeeded.",
                    );
                }
                self.last_load_error = None;
                self.context = Some(context);
            }
            Err(error) => {
                let summary = error.to_string();
                if self.last_load_error.as_deref() != Some(summary.as_str()) {
                    let _ =
                        write_log_line(&paths.component_log("lowlevel-init"), "ERROR", &summary);
                }
                self.last_load_error = Some(summary);
            }
        }
    }

    fn should_retry_load(&self) -> bool {
        self.last_load_attempt
            .map(|instant| instant.elapsed() >= LOAD_RETRY_INTERVAL)
            .unwrap_or(true)
    }
}

#[derive(Clone, Copy)]
struct RaplEnergySample {
    raw_counter: u32,
    energy_unit_j: f64,
    sampled_at: Instant,
}
