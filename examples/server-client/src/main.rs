#![cfg_attr(windows, windows_subsystem = "windows")]

//! The notes client: start `server-demo`, then `cargo run -p server-client`
//! (or pass the server's base URL).

use rustnative_core::{Application, Component, Size, Window};
use server_client::{Connection, NotesClient};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::args().nth(1).unwrap_or_else(|| "http://127.0.0.1:8080".into());
    let connection = Connection { base, name: "grace".into(), password: "compiler".into() };
    let identity = rustnative_host::AppIdentity::new("dev.rustnative.notes-client", "Notes");
    let mut application = Application::with_services(
        NotesClient::new(connection),
        Window::new("Notes", Size::new(520, 600)),
        rustnative_host::services(&identity)?,
    );
    rustnative_host::run(&mut application, &identity)?;
    Ok(())
}
