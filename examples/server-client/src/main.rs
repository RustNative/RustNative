#![cfg_attr(windows, windows_subsystem = "windows")]

//! The notes client: start `server-demo`, then `cargo run -p server-client`
//! (or pass the server's base URL).

use std::sync::Arc;

use rustnative_core::{Application, Component, Platform, Services, Size, Window};
#[cfg(target_os = "linux")]
use rustnative_linux::{LinuxPlatform as HostPlatform, SoupHttp as Http};
#[cfg(not(target_os = "linux"))]
use rustnative_windows::{WinHttp as Http, WindowsPlatform as HostPlatform};
use server_client::{Connection, NotesClient};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::args().nth(1).unwrap_or_else(|| "http://127.0.0.1:8080".into());
    let connection = Connection { base, name: "grace".into(), password: "compiler".into() };
    let mut application = Application::with_services(
        NotesClient::new(connection),
        Window::new("Notes", Size::new(520, 600)),
        Services::default().with_http(Arc::new(Http::new())),
    );
    HostPlatform::new().run(&mut application)?;
    Ok(())
}
