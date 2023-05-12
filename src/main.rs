mod server;
#[cfg(target_os = "windows")]
mod service;
use std::sync::{mpsc::channel, Arc, Mutex, RwLock};
use std::thread;

use actix_web::dev::Server;

fn main() {
    #[cfg(target_os = "windows")]
    {
        let args: Vec<String> = std::env::args().collect();
        match args.get(0) {
            Some(action) => {
                if action == "service" {
                    return service::main();
                };
            }
            None => {}
        };
    }

    // Static (/static) and config file is read from current directory on command line
    let cwd = std::env::current_dir().unwrap();
    let cwd_str: &str = &cwd.as_os_str().to_str().unwrap();

    // Hold server instance in a thread-safe RwLock
    let server_mutex: Arc<RwLock<Option<Server>>> = Arc::new(RwLock::new(Option::<Server>::None));

    // Server startup channel
    let (_tx, _rx) = channel::<Server>();
    thread::spawn(move || {
        // Wait for server startup
        match _rx.recv() {
            Ok(instance) => {
                // Persist server instance
                *server_mutex.write().unwrap() = Some(instance);
            }
            Err(err) => println!("{:?}", err),
        };
        ()
    });

    // Run server (this is a blocking call)
    server::serve(Box::new(cwd_str.to_string()), Arc::new(Mutex::new(_tx))).unwrap();
}
