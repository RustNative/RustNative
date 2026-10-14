#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::Arc;
use std::time::Duration;

use data_demo::{Board, DemoServer};
use rustnative_core::{Application, Component, Size, Window};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The server answers after 400 ms, so the loading and optimistic states
    // are visible.
    let server = DemoServer::new(12, Duration::from_millis(400));
    // The host's services — among them its state files, where queued
    // mutations survive a restart — with the in-process server as HTTP.
    let identity = rustnative_host::AppIdentity::new("RustNative.DataDemo", "Rust Native data");
    let services = rustnative_host::services(&identity)?.with_http(Arc::new(server.clone()));
    let mut application = Application::with_services(
        Board::new(server),
        Window::new("Rust Native data", Size::new(560, 520)),
        services,
    );
    rustnative_host::run(&mut application, &identity)?;
    Ok(())
}
