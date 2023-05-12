use std::sync::{mpsc::channel, Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use actix_web::dev::Server;
use futures::executor;

use crate::server;

extern crate windows_service;

use std::ffi::OsString;

use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::{define_windows_service, service_control_handler, service_dispatcher};

define_windows_service!(ffi_service_main, service_main);

const SERVICE_NAME: &str = "tmd-viewer-service";

fn service_main(_arguments: Vec<OsString>) {
    // Hold server instance, and windows service handle in a thread-safe RwLock
    let server_rwlock = RwLock::new(Option::<Server>::None);
    let server_ref = Arc::new(server_rwlock);
    let service_handle_rwlock = RwLock::new(Option::<ServiceStatusHandle>::None);
    let service_handle_ref = Arc::new(service_handle_rwlock);

    // Static (/static) and config file (tmd-viewer.yaml) is read from directory containing this exe.
    // Windows service sets cwd to C:/Windows/System/Win32 so we are overriding this on the server.
    let exe = std::env::current_exe().unwrap();
    let exe_dir = exe.parent().unwrap();
    let exe_dir_str: &str = &exe_dir.as_os_str().to_str().unwrap();

    // The entry point where execution will start on a background thread after a call to
    // `service_dispatcher::start` from `main`.
    let server_ref_eh = server_ref.clone();
    let service_handle_ref_eh = service_handle_ref.clone();
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            // Handle stop event and return control back to the system.
            ServiceControl::Stop => match server_ref_eh.read().unwrap().as_ref() {
                Some(instance) => {
                    // Update windows service status to pending stop
                    let stop_pending_status = ServiceStatus {
                        service_type: ServiceType::OWN_PROCESS,
                        current_state: ServiceState::StopPending,
                        controls_accepted: ServiceControlAccept::STOP,
                        exit_code: ServiceExitCode::Win32(0),
                        checkpoint: 0,
                        wait_hint: Duration::from_secs(10),
                        process_id: None,
                    };
                    match service_handle_ref_eh.read().unwrap().as_ref() {
                        Some(handle) => handle.set_service_status(stop_pending_status).unwrap(),
                        None => {}
                    };

                    // Stop service synchronously
                    executor::block_on(instance.stop(true));

                    // Update windows service status to stopped
                    let stopped_status = ServiceStatus {
                        service_type: ServiceType::OWN_PROCESS,
                        current_state: ServiceState::Stopped,
                        controls_accepted: ServiceControlAccept::STOP,
                        exit_code: ServiceExitCode::Win32(0),
                        checkpoint: 0,
                        wait_hint: Duration::from_secs(10),
                        process_id: None,
                    };
                    match service_handle_ref_eh.read().unwrap().as_ref() {
                        Some(handle) => handle.set_service_status(stopped_status).unwrap(),
                        None => {}
                    };
                    ServiceControlHandlerResult::NoError
                }
                None => ServiceControlHandlerResult::NoError,
            },
            // All services must accept Interrogate even if it's a no-op.
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            // ???
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    // Register system service event handler
    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler).unwrap();
    *service_handle_ref.write().unwrap() = Some(status_handle);

    // Server start channel
    let (start_tx, start_rx) = channel::<Server>();
    let server_ref_start_rx = server_ref.clone();
    let service_handle_ref_start_rx = service_handle_ref.clone();
    thread::spawn(move || {
        // Wait for server startup
        match start_rx.recv() {
            Ok(instance) => {
                // Persist server instance to server_ref
                *server_ref_start_rx.write().unwrap() = Some(instance);

                // Update windows service status to running
                let running_status = ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: ServiceState::Running,
                    controls_accepted: ServiceControlAccept::STOP,
                    exit_code: ServiceExitCode::Win32(0),
                    checkpoint: 0,
                    wait_hint: Duration::from_secs(10),
                    process_id: None,
                };
                match service_handle_ref_start_rx.read().unwrap().as_ref() {
                    Some(handle) => handle.set_service_status(running_status).unwrap(),
                    None => {}
                };
            }
            Err(err) => println!("{:?}", err),
        };
        ()
    });

    // Run server (this is a blocking call)
    server::serve(
        Box::new(exe_dir_str.to_string()),
        Arc::new(Mutex::new(start_tx)),
    )
    .unwrap();
}

pub(crate) fn main() {
    // Register generated `ffi_service_main` with the system and start the service, blocking
    // this thread until the service is stopped.
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).unwrap();
}
