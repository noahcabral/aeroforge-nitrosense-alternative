mod capability;
mod control;
mod ipc;
mod lowlevel;
mod persistence;
mod telemetry;

use std::{
    collections::BTreeMap,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    sync::{atomic::AtomicBool, Arc},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::paths::{write_log_line, ServicePaths};

pub struct ServiceHost {
    paths: ServicePaths,
    stop_flag: Arc<AtomicBool>,
}

impl ServiceHost {
    pub fn new(paths: ServicePaths, stop_flag: Arc<AtomicBool>) -> Self {
        Self { paths, stop_flag }
    }

    pub fn start(self) -> Result<Vec<JoinHandle<()>>, Box<dyn std::error::Error + Send + Sync>> {
        let mut handles = Vec::new();
        let (event_tx, event_rx) = mpsc::channel();

        handles.push(spawn_worker(
            capability::registration(),
            self.paths.clone(),
            self.stop_flag.clone(),
            event_tx.clone(),
        )?);

        handles.push(spawn_worker(
            control::registration(),
            self.paths.clone(),
            self.stop_flag.clone(),
            event_tx.clone(),
        )?);

        handles.push(spawn_worker(
            persistence::registration(),
            self.paths.clone(),
            self.stop_flag.clone(),
            event_tx.clone(),
        )?);

        handles.push(spawn_worker(
            lowlevel::registration(),
            self.paths.clone(),
            self.stop_flag.clone(),
            event_tx.clone(),
        )?);

        handles.push(spawn_worker(
            telemetry::registration(),
            self.paths.clone(),
            self.stop_flag.clone(),
            event_tx.clone(),
        )?);

        handles.push(spawn_worker(
            ipc::registration(),
            self.paths.clone(),
            self.stop_flag.clone(),
            event_tx.clone(),
        )?);

        drop(event_tx);

        handles.push(spawn_supervisor(
            self.paths.clone(),
            self.stop_flag.clone(),
            event_rx,
        )?);

        Ok(handles)
    }
}

pub struct WorkerRegistration {
    pub name: &'static str,
    pub runner: fn(
        ServicePaths,
        Arc<AtomicBool>,
        WorkerEventSender,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>,
}

impl WorkerRegistration {
    pub const fn new(
        name: &'static str,
        runner: fn(
            ServicePaths,
            Arc<AtomicBool>,
            WorkerEventSender,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self { name, runner }
    }
}

#[derive(Clone)]
pub struct WorkerEvent {
    pub worker: &'static str,
    pub state: WorkerState,
    pub message: Option<String>,
    pub interval_seconds: u64,
    pub timestamp_unix: u64,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkerState {
    Starting,
    Running,
    Failed,
    Stopped,
}

pub type WorkerEventSender = Sender<WorkerEvent>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupervisorSnapshot {
    service: &'static str,
    worker_count: usize,
    updated_at_unix: u64,
    workers: Vec<WorkerStatusSnapshot>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkerStatusSnapshot {
    name: String,
    state: WorkerState,
    interval_seconds: u64,
    last_update_unix: u64,
    last_error: Option<String>,
}

fn spawn_worker(
    registration: WorkerRegistration,
    paths: ServicePaths,
    stop_flag: Arc<AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    let name = registration.name;
    write_log_line(&paths.service_log(), "INFO", &format!("Starting {name}."))?;
    write_log_line(
        &paths.component_log(name),
        "INFO",
        &format!("Starting {name}."),
    )?;

    Ok(thread::Builder::new().name(name.into()).spawn(move || {
        let _ = event_tx.send(WorkerEvent {
            worker: name,
            state: WorkerState::Starting,
            message: None,
            interval_seconds: 0,
            timestamp_unix: unix_timestamp(),
        });

        if let Err(error) = (registration.runner)(paths.clone(), stop_flag, event_tx.clone()) {
            let message = error.to_string();
            let _ = write_log_line(
                &paths.service_log(),
                "ERROR",
                &format!("{name} failed: {message}"),
            );
            let _ = write_log_line(
                &paths.component_log(name),
                "ERROR",
                &format!("{name} failed: {message}"),
            );
            let _ = event_tx.send(WorkerEvent {
                worker: name,
                state: WorkerState::Failed,
                message: Some(message),
                interval_seconds: 0,
                timestamp_unix: unix_timestamp(),
            });
        }

        let _ = event_tx.send(WorkerEvent {
            worker: name,
            state: WorkerState::Stopped,
            message: None,
            interval_seconds: 0,
            timestamp_unix: unix_timestamp(),
        });
        let _ = write_log_line(
            &paths.component_log(name),
            "INFO",
            &format!("{name} stopped."),
        );
    })?)
}

fn spawn_supervisor(
    paths: ServicePaths,
    stop_flag: Arc<AtomicBool>,
    event_rx: Receiver<WorkerEvent>,
) -> Result<JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    write_log_line(
        &paths.service_log(),
        "INFO",
        "Starting supervisor snapshot loop.",
    )?;

    Ok(thread::Builder::new()
        .name("supervisor-worker".into())
        .spawn(move || {
            let mut workers = BTreeMap::<String, WorkerStatusSnapshot>::new();

            loop {
                match event_rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(event) => {
                        workers.insert(
                            event.worker.to_string(),
                            WorkerStatusSnapshot {
                                name: event.worker.to_string(),
                                state: event.state,
                                interval_seconds: event.interval_seconds,
                                last_update_unix: event.timestamp_unix,
                                last_error: matches!(event.state, WorkerState::Failed)
                                    .then_some(event.message)
                                    .flatten(),
                            },
                        );
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
                            let _ = persist_supervisor_snapshot(&paths, &workers);
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        let _ = persist_supervisor_snapshot(&paths, &workers);
                        break;
                    }
                }

                let _ = persist_supervisor_snapshot(&paths, &workers);
            }
        })?)
}

fn persist_supervisor_snapshot(
    paths: &ServicePaths,
    workers: &BTreeMap<String, WorkerStatusSnapshot>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let snapshot = SupervisorSnapshot {
        service: "AeroForgeService",
        worker_count: workers.len(),
        updated_at_unix: unix_timestamp(),
        workers: workers.values().cloned().collect(),
    };

    std::fs::write(
        paths.supervisor_snapshot(),
        serde_json::to_string_pretty(&snapshot)?,
    )?;

    Ok(())
}

pub fn wake_ipc_listener() {
    ipc::wake_listener();
}

pub fn run_periodic_worker(
    name: &'static str,
    interval: Duration,
    paths: ServicePaths,
    stop_flag: Arc<AtomicBool>,
    event_tx: WorkerEventSender,
    tick: fn(&ServicePaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    while !stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
        tick(&paths)?;
        let _ = event_tx.send(WorkerEvent {
            worker: name,
            state: WorkerState::Running,
            message: None,
            interval_seconds: interval.as_secs(),
            timestamp_unix: unix_timestamp(),
        });
        sleep_until_next_tick(interval, &stop_flag);
    }

    Ok(())
}

pub fn sleep_until_next_tick(interval: Duration, stop_flag: &Arc<AtomicBool>) {
    let slice = Duration::from_millis(250);
    let mut remaining = interval;

    while remaining > Duration::ZERO {
        if stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }

        let sleep_for = remaining.min(slice);
        thread::sleep(sleep_for);
        remaining = remaining.saturating_sub(sleep_for);
    }
}

pub fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
