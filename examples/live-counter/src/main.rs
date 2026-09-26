#![cfg_attr(windows, windows_subsystem = "windows")]

//! `live-counter serve 8095` runs a server instance; `live-counter` opens
//! the Windows client on `ws://127.0.0.1:8095`. Stop and restart the server
//! within 30 seconds: the count survives.

use std::time::Duration;

use live_counter::CounterApp;
use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_sync::live::{LiveClient, LiveServer, RemoteView};
use rustnative_windows::WindowsPlatform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let runtime = tokio::runtime::Runtime::new()?;
    if arguments.next().as_deref() == Some("serve") {
        let port = arguments.next().unwrap_or_else(|| "8095".into());
        return runtime.block_on(async {
            let server = LiveServer::new(CounterApp, Duration::from_secs(30));
            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
            server.serve(listener).await;
            Ok(())
        });
    }
    // The connection is kept up on the runtime's threads.
    let _inside = runtime.enter();
    let client = LiveClient::connect("ws://127.0.0.1:8095");
    let mut application =
        Application::new(RemoteView::new(client), Window::new("Live counter", Size::new(360, 240)));
    WindowsPlatform::new().run(&mut application)?;
    Ok(())
}
