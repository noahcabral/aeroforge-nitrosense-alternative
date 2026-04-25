mod handlers;
mod models;
mod pipe;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use models::{PipeRequest, PipeResponse};

use crate::{
    paths::ServicePaths,
    workers::{unix_timestamp, WorkerEvent, WorkerEventSender, WorkerRegistration, WorkerState},
};

const PIPE_PATH: &str = r"\\.\pipe\AeroForgeService";

pub fn registration() -> WorkerRegistration {
    WorkerRegistration::new("ipc-worker", run)
}

pub fn wake_listener() {
    let _ = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_PATH);
}

pub fn run(
    paths: ServicePaths,
    stop_flag: Arc<AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = event_tx.send(WorkerEvent {
        worker: "ipc-worker",
        state: WorkerState::Running,
        message: Some(format!("Listening on {PIPE_PATH}.")),
        interval_seconds: 0,
        timestamp_unix: unix_timestamp(),
    });

    while !stop_flag.load(Ordering::SeqCst) {
        let pipe = pipe::create_pipe_instance(&paths, PIPE_PATH)?;
        pipe::connect_client(&pipe)?;
        handle_client(pipe, &paths)?;

        let _ = event_tx.send(WorkerEvent {
            worker: "ipc-worker",
            state: WorkerState::Running,
            message: Some("Handled named-pipe request.".into()),
            interval_seconds: 0,
            timestamp_unix: unix_timestamp(),
        });
    }

    Ok(())
}

fn handle_client(
    pipe: pipe::NamedPipeInstance,
    paths: &ServicePaths,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut file = pipe.into_file();
    let mut reader = std::io::BufReader::new(&mut file);
    let mut line = String::new();
    let bytes_read = std::io::BufRead::read_line(&mut reader, &mut line)?;

    if bytes_read == 0 || line.trim().is_empty() {
        return Ok(());
    }

    let response = match serde_json::from_str::<PipeRequest>(&line) {
        Ok(request) => handlers::process_request(request, paths, PIPE_PATH),
        Err(error) => PipeResponse::Error {
            message: format!("Invalid pipe request: {error}"),
        },
    };

    drop(reader);
    let serialized = serde_json::to_string(&response)?;
    std::io::Write::write_all(&mut file, serialized.as_bytes())?;
    std::io::Write::write_all(&mut file, b"\n")?;
    std::io::Write::flush(&mut file)?;
    Ok(())
}
