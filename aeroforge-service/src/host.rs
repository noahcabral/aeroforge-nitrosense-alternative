use std::{
    ffi::OsString,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers,
};

define_windows_service!(ffi_service_main, service_main);

const SERVICE_WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run_console_host() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let paths = ServicePaths::discover()?;
    let shutdown_log = paths.clone();
    write_log_line(
        &paths.service_log(),
        "INFO",
        "Starting AeroForge service host in console mode.",
    )?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let ctrlc_stop = stop_flag.clone();
    ctrlc::set_handler(move || {
        ctrlc_stop.store(true, Ordering::SeqCst);
    })?;

    let host = workers::ServiceHost::new(paths, stop_flag.clone());
    let join_handles = host.start()?;

    loop {
        if stop_flag.load(Ordering::SeqCst) {
            break;
        }

        thread::sleep(Duration::from_secs(1));
    }

    workers::wake_ipc_listener();

    for handle in join_handles {
        let _ = handle.join();
    }

    write_log_line(
        &shutdown_log.service_log(),
        "INFO",
        "Console-mode service host stopped.",
    )?;

    Ok(())
}

pub fn run_windows_service_host(
    service_name: &'static str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    service_dispatcher::start(service_name, ffi_service_main)?;
    Ok(())
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service() {
        let _ = eprintln!("AeroForge service failed: {error}");
    }
}

fn run_service() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let paths = ServicePaths::discover()?;
    write_log_line(
        &paths.service_log(),
        "INFO",
        "Starting AeroForge Windows service host.",
    )?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_signal = stop_flag.clone();
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let status_handle =
        service_control_handler::register(
            SERVICE_NAME,
            move |control_event| match control_event {
                ServiceControl::Stop => {
                    stop_signal.store(true, Ordering::SeqCst);
                    let _ = shutdown_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            },
        )?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let host = workers::ServiceHost::new(paths.clone(), stop_flag.clone());
    let join_handles = host.start()?;

    let _ = shutdown_rx.recv();

    write_log_line(
        &paths.service_log(),
        "INFO",
        "Stop signal received. Shutting down workers.",
    )?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::StopPending,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: SERVICE_WORKER_SHUTDOWN_TIMEOUT,
        process_id: None,
    })?;

    workers::wake_ipc_listener();

    wait_for_worker_shutdown(join_handles, SERVICE_WORKER_SHUTDOWN_TIMEOUT, &paths);

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

fn wait_for_worker_shutdown(
    join_handles: Vec<thread::JoinHandle<()>>,
    timeout: Duration,
    paths: &ServicePaths,
) {
    let worker_count = join_handles.len();
    let (done_tx, done_rx) = mpsc::channel::<()>();

    for handle in join_handles {
        let done_tx = done_tx.clone();
        let _ = thread::Builder::new()
            .name("worker-shutdown-join".into())
            .spawn(move || {
                let _ = handle.join();
                let _ = done_tx.send(());
            });
    }
    drop(done_tx);

    let deadline = Instant::now() + timeout;
    let mut finished = 0usize;

    while finished < worker_count {
        let now = Instant::now();
        if now >= deadline {
            break;
        }

        match done_rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(()) => finished += 1,
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if finished < worker_count {
        let _ = write_log_line(
            &paths.service_log(),
            "WARN",
            &format!(
                "Service shutdown timed out after {} seconds; {finished}/{worker_count} workers reported stopped. Continuing service stop so updates can replace the binary.",
                timeout.as_secs()
            ),
        );
    } else {
        let _ = write_log_line(
            &paths.service_log(),
            "INFO",
            "All service workers stopped cleanly.",
        );
    }
}

const SERVICE_NAME: &str = "AeroForgeService";
