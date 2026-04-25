mod models;
mod state;

use models::PersistenceSnapshot;

use crate::{
    paths::ServicePaths,
    workers::{run_periodic_worker, WorkerEventSender, WorkerRegistration},
};

const WORKER_NAME: &str = "persistence-worker";
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

pub fn registration() -> WorkerRegistration {
    WorkerRegistration::new(WORKER_NAME, run)
}

fn run(
    paths: ServicePaths,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_periodic_worker(
        WORKER_NAME,
        SAMPLE_INTERVAL,
        paths,
        stop_flag,
        event_tx,
        tick,
    )
}

fn tick(paths: &ServicePaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tauri_config_path = state::discover_tauri_state_path();
    let snapshot = PersistenceSnapshot {
        service: WORKER_NAME,
        tauri_control_state_path: tauri_config_path.display().to_string(),
        tauri_control_state_present: tauri_config_path.exists(),
        service_state_root: paths.state_dir.display().to_string(),
        next_phase:
            "Move AeroForge-owned settings behind a shared service contract instead of separate app-owned files.",
    };

    std::fs::write(
        paths.worker_snapshot("persistence"),
        serde_json::to_string_pretty(&snapshot)?,
    )?;

    Ok(())
}
