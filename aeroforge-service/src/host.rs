use std::{
    ffi::OsString,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
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

    workers::wake_ipc_listener();

    for handle in join_handles {
        let _ = handle.join();
    }

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

const SERVICE_NAME: &str = "AeroForgeService";
