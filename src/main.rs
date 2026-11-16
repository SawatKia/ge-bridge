#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use ge_bridge::{config, server, state, tray};

use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = config::init(config::RuntimeConfig::from_env());

    // `ge-bridge.exe --acp` is kept for old configs; it runs the same ACP agent as `ge-bridge-cli acp`.
    if args.iter().any(|a| a == "--acp") {
        let _ = ge_bridge::acp::run();
        return;
    }

    // Prevent duplicate tray instances on Windows; open Dashboard if already running
    if cfg.data_dir.is_none() && !acquire_single_instance_lock() {
        let _ = open::that("http://127.0.0.1:8787");
        return;
    }

    let state = Arc::new(state::AppState::load_or_init());
    let server_state = state.clone();
    state.log(
        "info",
        format!(
            "ge-bridge {} starting: port={} data_dir={} tray={}",
            env!("CARGO_PKG_VERSION"),
            state.prefs.read().port,
            state::PersistedPrefs::app_dir().display(),
            if cfg.no_tray { "disabled" } else { "enabled" }
        ),
    );

    // Background 2-thread Tokio async runtime (~3.5 MB RAM)
    let server_thread = std::thread::Builder::new()
        .name("ge-bridge-tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("Failed to start Tokio runtime");
            rt.block_on(async move {
                if let Err(err) = server::run_http_server(server_state).await {
                    eprintln!("Server error: {err}");
                }
            });
        })
        .expect("Failed to spawn Tokio server thread");

    if cfg.no_tray {
        // Headless: keep the process alive for as long as the server runs.
        let _ = server_thread.join();
        return;
    }

    // Main Thread: Native Win32 System-Tray message loop
    tray::run_tray_loop(state);
}

#[cfg(windows)]
fn acquire_single_instance_lock() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Local\\GeBridgeSingleton_v2\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        !handle.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn acquire_single_instance_lock() -> bool {
    true
}
