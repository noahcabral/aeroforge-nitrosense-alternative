mod models;
mod topology;
mod winring;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use models::LowLevelSnapshot;

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
}

impl LowLevelSampler {
    fn new() -> Self {
        Self {
            context: None,
            last_load_attempt: None,
            last_load_error: None,
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

        let mut core_temps = Vec::with_capacity(core_affinity_masks.len());
        for mask in core_affinity_masks.iter().copied() {
            if let Ok(Some(temp_c)) = context.read_core_temp(mask, tj_max_c) {
                core_temps.push(temp_c);
            }
        }

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
                context.driver_path.display(),
                sampled_processor_count,
                logical_processor_count
            ),
            driver_path: Some(context.driver_path.display().to_string()),
            logical_processor_count,
            sampled_processor_count,
            tj_max_c,
            package_temp_c,
            average_core_temp_c,
            lowest_core_temp_c,
            highest_core_temp_c,
            core_temps_c: core_temps,
            hottest_cores_c,
        })
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
