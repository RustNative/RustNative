#![cfg_attr(windows, windows_subsystem = "windows")]

//! Collaborative notes. `collab-notes serve` runs the sync server on
//! 127.0.0.1:8090; `collab-notes <replica>` opens an editor that syncs with
//! it through the host's HTTP stack. Start two editors, disconnect one,
//! edit both, reconnect, and press Sync.

use std::sync::{Arc, Mutex};

use collab_notes::{Doc, Editor, EditorProps, merge_policy};
use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_server::ServerApp;
use rustnative_sync::SyncServer;
use rustnative_sync::http::HttpSync;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argument = std::env::args().nth(1).unwrap_or_else(|| "1".into());
    if argument == "serve" {
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(async {
            let sync = Arc::new(Mutex::new(SyncServer::new().collection::<Doc>(merge_policy(), 1)));
            let app = rustnative_sync::http::server::mount(ServerApp::new(), &sync, |router| {
                router.csrf_exempt().public()
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:8090").await?;
            app.into_service().serve(listener, std::future::pending()).await?;
            Ok(())
        });
    }
    let replica = argument.parse().unwrap_or(1);
    let identity = rustnative_host::AppIdentity::new(
        format!("dev.rustnative.collab-notes-{replica}"),
        "Notes",
    );
    let services = rustnative_host::services(&identity)?;
    let http = services.http().cloned().ok_or("this host has no HTTP service")?;
    let transport = Arc::new(HttpSync::new(http, "http://127.0.0.1:8090", Vec::new()));
    let mut application = Application::with_services(
        Editor::new(EditorProps { replica, doc: "shopping".into(), transport }),
        Window::new("Collaborative notes", Size::new(420, 320)),
        services,
    );
    // Each replica is its own instance: no single-instance hand-off.
    rustnative_host::HostPlatform::new().run(&mut application)?;
    Ok(())
}
