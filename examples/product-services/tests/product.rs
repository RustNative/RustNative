//! The product-services example on the headless backend: what it asks of
//! its surfaces, the tray's actions, its token in secure storage across a
//! restart, and a remotely toggled feature.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::Arc;

use product_services::{Product, TOKEN};
use rustnative_core::capability::SurfaceKind;
use rustnative_core::product::{Flags, MemorySecureStorage, SecureStorage};
use rustnative_core::surfaces::{NOTIFICATION, SurfaceCommand};
use rustnative_core::{Component, Event, Services, Size, Theme, Window, WindowId};
use rustnative_headless::{HeadlessApp, Query};

fn launch(services: Services) -> HeadlessApp {
    HeadlessApp::launch_with(
        Window::new("product", Size::new(480, 320)),
        services,
        Theme::default(),
        || Product::new(()),
    )
}

fn text(app: &HeadlessApp, key: &str) -> String {
    app.find(&Query::key(key)).unwrap().text.clone().unwrap_or_default()
}

#[test]
fn the_tray_jump_list_progress_and_notification_are_asked_for() {
    let services = Services::default();
    let surfaces = services.surfaces().clone();
    let mut app = launch(services);
    let started = surfaces.take();
    assert!(matches!(&started[0], SurfaceCommand::ShowTray { menu, .. } if menu[0].id == "export"));
    assert!(
        matches!(&started[1], SurfaceCommand::JumpList(tasks) if tasks[0].arguments == "product://new")
    );

    for _ in 0..3 {
        app.click(&Query::key("export")).unwrap();
    }
    // The tray menu's "Export" does what the button does.
    app.dispatch(Event::SurfaceAction {
        window: WindowId::PRIMARY,
        surface: SurfaceKind::TrayExtra,
        action: "export".into(),
    });
    assert_eq!(
        surfaces.take(),
        [
            SurfaceCommand::Progress(Some(0.25)),
            SurfaceCommand::Progress(Some(0.5)),
            SurfaceCommand::Progress(Some(0.75)),
            SurfaceCommand::Progress(None),
            SurfaceCommand::Notify {
                title: "Export finished".into(),
                body: "Your notes were exported.".into()
            },
        ]
    );
    app.dispatch(Event::SurfaceAction {
        window: WindowId::PRIMARY,
        surface: SurfaceKind::TrayExtra,
        action: NOTIFICATION.into(),
    });
    assert_eq!(text(&app, "status"), "You came back from the notification.");
}

#[test]
fn the_token_is_kept_in_secure_storage_across_a_restart() {
    let secrets = Arc::new(MemorySecureStorage::default());
    let mut app = launch(Services::default().with_secure_storage(secrets.clone()));
    app.click(&Query::key("sign-in")).unwrap();
    assert!(secrets.get(TOKEN).unwrap().is_some());
    drop(app);
    let app = launch(Services::default().with_secure_storage(secrets.clone()));
    assert!(app.find(&Query::key("sign-out")).is_ok(), "signed in from the stored token");
}

#[test]
fn a_remote_flag_turns_the_compact_layout_on() {
    let flags = Flags::default();
    let mut app = launch(Services::default().with_flags(flags.clone()));
    assert_eq!(text(&app, "title"), "Notes", "the compiled default");
    flags.apply_remote(&serde_json::json!({ "compact-layout": true }));
    app.click(&Query::key("export")).unwrap();
    assert_eq!(text(&app, "title"), "Notes (compact)");
}
