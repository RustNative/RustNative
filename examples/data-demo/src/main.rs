#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::Duration;

use data_demo::{Board, DemoServer};
use rustnative_core::{Application, Component, Platform, Services, Size, Window};
#[cfg(target_os = "linux")]
use rustnative_linux::FileStateStore;
#[cfg(target_os = "linux")]
use rustnative_linux::LinuxPlatform as HostPlatform;
#[cfg(windows)]
use rustnative_windows::FileStateStore;
#[cfg(not(target_os = "linux"))]
use rustnative_windows::WindowsPlatform as HostPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The server answers after 400 ms, so the loading and optimistic states
    // are visible.
    let server = DemoServer::new(12, Duration::from_millis(400));
    let services = Services::default().with_http(Arc::new(server.clone()));
    // Queued mutations survive a restart in the application's state files.
    #[cfg(any(windows, target_os = "linux"))]
    let services = match FileStateStore::for_app("RustNative.DataDemo") {
        Ok(store) => services.with_state_store(Arc::new(store)),
        Err(_) => services,
    };
    let mut application = Application::with_services(
        Board::new(server),
        Window::new("Rust Native data", Size::new(560, 520)),
        services,
    );
    HostPlatform::new().run(&mut application)?;
    Ok(())
}
