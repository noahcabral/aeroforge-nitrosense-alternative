mod handlers;
mod models;
mod pipe;

use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use models::{PipeRequest, PipeResponse};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::{unix_timestamp, WorkerEvent, WorkerEventSender, WorkerRegistration, WorkerState},
};

const PIPE_PATH: &str = r"\\.\pipe\AeroForgeService";
/// Requests are small JSON lines (boot-logo applies carry a path, not the
/// image); this is a generous ceiling, not a sizing.
const MAX_REQUEST_BYTES: u64 = 32 * 1024 * 1024;
/// Any authenticated local user can connect, so bound what one can pin.
/// ponytail: no read timeout on the pipe file, so an idle client still holds a
/// slot until it disconnects; move to overlapped I/O if that is ever abused.
const MAX_CONCURRENT_CLIENTS: usize = 16;
static ACTIVE_CLIENTS: AtomicUsize = AtomicUsize::new(0);

/// Holds one of the MAX_CONCURRENT_CLIENTS slots; released on drop, which also
/// covers thread-spawn failure and a panicking handler.
struct ClientSlot;

impl ClientSlot {
    fn acquire() -> Option<Self> {
        if ACTIVE_CLIENTS.fetch_add(1, Ordering::SeqCst) >= MAX_CONCURRENT_CLIENTS {
            ACTIVE_CLIENTS.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self)
    }
}

impl Drop for ClientSlot {
    fn drop(&mut self) {
        ACTIVE_CLIENTS.fetch_sub(1, Ordering::SeqCst);
    }
}

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
        let Some(slot) = ClientSlot::acquire() else {
            drop(pipe);
            let _ = write_log_line(
                &paths.component_log("ipc-worker"),
                "WARN",
                "Rejected named-pipe client: too many concurrent connections.",
            );
            continue;
        };
        let client_paths = paths.clone();
        std::thread::Builder::new()
            .name("ipc-client".into())
            .spawn(move || {
                let _slot = slot;
                if let Err(error) = handle_client(pipe, &client_paths) {
                    let _ = write_log_line(
                        &client_paths.component_log("ipc-worker"),
                        "ERROR",
                        &format!("Named-pipe client handler failed: {error}"),
                    );
                }
            })?;

        let _ = event_tx.send(WorkerEvent {
            worker: "ipc-worker",
            state: WorkerState::Running,
            message: Some("Accepted named-pipe request.".into()),
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
    let mut reader = std::io::BufReader::new((&mut file).take(MAX_REQUEST_BYTES));
    let mut line = String::new();
    let bytes_read = std::io::BufRead::read_line(&mut reader, &mut line)?;

    if bytes_read == 0 || line.trim().is_empty() {
        return Ok(());
    }
    if !line.ends_with('\n') {
        return Err(format!("Named-pipe request exceeded {MAX_REQUEST_BYTES} bytes.").into());
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
