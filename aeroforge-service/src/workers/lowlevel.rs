mod models;
pub(crate) mod pawnio;
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
use winring::{PackagePowerLimitApplyResult, PackagePowerLimitWrite, RaplReadback};

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

pub(crate) enum MsrProvider {
    PawnIo(pawnio::PawnIoContext),
    WinRing(winring::WinRingContext),
}

impl MsrProvider {
    fn transport(&self) -> &'static str {
        match self {
            Self::PawnIo(_) => "pawnio",
            Self::WinRing(_) => "winring0",
        }
    }

    fn source_path(&self) -> String {
        match self {
            Self::PawnIo(context) => context.module_path.display().to_string(),
            Self::WinRing(context) => context.driver_path.display().to_string(),
        }
    }

    fn detail(&self, sampled_processor_count: usize, logical_processor_count: usize) -> String {
        match self {
            Self::PawnIo(context) => format!(
                "PawnIO MSR provider active from {}. RAPL telemetry is sampled through a restricted AeroForge module across {} logical processors.",
                context.module_path.display(),
                logical_processor_count
            ),
            Self::WinRing(context) => format!(
                "WinRing0 kernel driver active from {}. Sampled {} physical cores across {} logical processors.",
                context.driver_path.display(),
                sampled_processor_count,
                logical_processor_count
            ),
        }
    }

    fn read_tj_max(&self) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Self::PawnIo(_) => Ok(None),
            Self::WinRing(context) => context.read_tj_max(),
        }
    }

    fn read_package_temp(
        &self,
        tj_max_c: Option<u8>,
    ) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Self::PawnIo(_) => Ok(None),
            Self::WinRing(context) => context.read_package_temp(tj_max_c),
        }
    }

    fn read_core_temp(
        &self,
        affinity_mask: usize,
        tj_max_c: Option<u8>,
    ) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Self::PawnIo(_) => Ok(None),
            Self::WinRing(context) => context.read_core_temp(affinity_mask, tj_max_c),
        }
    }

    pub(crate) fn read_rapl_readback(
        &self,
    ) -> Result<Option<RaplReadback>, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Self::PawnIo(context) => context.read_rapl_readback(),
            Self::WinRing(context) => context.read_rapl_readback(),
        }
    }

    pub(crate) fn apply_package_power_limit(
        &self,
        write: PackagePowerLimitWrite,
    ) -> Result<Option<PackagePowerLimitApplyResult>, Box<dyn std::error::Error + Send + Sync>>
    {
        match self {
            Self::PawnIo(context) => context.apply_package_power_limit(write),
            Self::WinRing(context) => context.apply_package_power_limit(write),
        }
    }
}

pub(crate) fn load_msr_provider(
    paths: &ServicePaths,
) -> Result<MsrProvider, Box<dyn std::error::Error + Send + Sync>> {
    match pawnio::PawnIoContext::load(paths) {
        Ok(context) => return Ok(MsrProvider::PawnIo(context)),
        Err(pawnio_error) => match winring::WinRingContext::load(paths) {
            Ok(context) => Ok(MsrProvider::WinRing(context)),
            Err(winring_error) => Err(format!(
                "No CPU MSR/RAPL provider is available. PawnIO: {pawnio_error} WinRing0: {winring_error}"
            )
            .into()),
        },
    }
}

struct LowLevelSampler {
    context: Option<MsrProvider>,
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

        let source_path = context.source_path();
        let sampled_processor_count = core_temps.len();
        let transport = context.transport().to_string();
        let detail = context.detail(sampled_processor_count, logical_processor_count);
        let package_power_w = self.calculate_package_power_w(rapl_readback);
        let power_limit = rapl_readback.and_then(|readback| readback.package_power_limit);
        let average_core_temp_c = models::hottest_core_average(&core_temps, 3);
        let lowest_core_temp_c = core_temps.iter().copied().min();
        let highest_core_temp_c = core_temps.iter().copied().max();
        let hottest_cores_c = models::hottest_cores(&core_temps, 3);

        Ok(LowLevelSnapshot {
            available: tj_max_c.is_some()
                || package_temp_c.is_some()
                || average_core_temp_c.is_some()
                || rapl_readback.is_some(),
            transport,
            detail,
            driver_path: Some(source_path),
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
        match load_msr_provider(paths) {
            Ok(context) => {
                if self.last_load_error.is_some() {
                    let _ = write_log_line(
                        &paths.component_log("lowlevel-init"),
                        "INFO",
                        "Recovered after prior CPU MSR/RAPL provider initialization failure.",
                    );
                } else {
                    let _ = write_log_line(
                        &paths.component_log("lowlevel-init"),
                        "INFO",
                        &format!(
                            "CPU MSR/RAPL provider initialized through {}.",
                            context.transport()
                        ),
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
