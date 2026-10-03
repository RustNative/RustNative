//! The services against the real GTK thread and fake freedesktop services
//! on the test's private bus (Phase 6).

use std::future::Future;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{
    ClipboardService, FileDialogKind, FileDialogRequest, FileDialogService, SystemService,
};

use super::testing::on_gtk;
use crate::desktop::fake_bus::{FakeService, Object};

/// Runs `future` on another thread (as a component's task would run) while
/// this — the GTK — thread keeps its main context going, and returns what
/// it produced.
fn drive<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    let (reply, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        let _ = reply.send(runtime.block_on(future));
    });
    let context = glib::MainContext::default();
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(value) = answer.try_recv() {
            return value;
        }
        assert!(Instant::now() < until, "the service did not answer");
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn the_clipboard_round_trips_through_gdk() {
    on_gtk(|| {
        drive(async { crate::LinuxClipboard.write_text("from a task".into()).await })
            .expect("written");
        let read = drive(async { crate::LinuxClipboard.read_text().await }).expect("read");
        assert_eq!(read.as_deref(), Some("from a task"));
    });
}

fn notification_server(calls: Arc<Mutex<u32>>) -> FakeService {
    let xml = r#"<node><interface name="org.freedesktop.Notifications">
        <method name="Notify">
          <arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/>
          <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/>
          <arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/>
        </method>
        <signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
        </interface></node>"#;
    FakeService::start(
        "org.freedesktop.Notifications",
        vec![Object {
            path: "/org/freedesktop/Notifications".into(),
            xml: xml.into(),
            handler: Arc::new(move |_, _| {
                let mut count = calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                *count += 1;
                Ok((40_u32 + *count,).to_variant())
            }),
        }],
    )
}

#[test]
fn a_notification_reaches_the_notification_server_and_its_click_comes_back() {
    on_gtk(|| {
        let server = notification_server(Arc::default());
        drive(async {
            crate::LinuxSystem::named("Notes")
                .notify("Saved".into(), "All changes saved".into())
                .await
        })
        .expect("delivered");
        let notify = server.calls_to("Notify");
        let call = notify.first().expect("one Notify call");
        let (application, _, _, title, body, _, _, _) = call
            .parameters
            .get::<(
                String,
                u32,
                String,
                String,
                String,
                Vec<String>,
                std::collections::HashMap<String, glib::Variant>,
                i32,
            )>()
            .expect("the Notify signature");
        assert_eq!(
            (application.as_str(), title.as_str(), body.as_str()),
            ("Notes", "Saved", "All changes saved")
        );

        // A notification with a click handler offers the default action,
        // and the server's ActionInvoked reaches the handler.
        let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = std::rc::Rc::clone(&clicked);
        let delivered = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = std::rc::Rc::clone(&delivered);
        glib::MainContext::default().spawn_local(async move {
            let on_click: Box<dyn Fn()> = Box::new(move || seen.set(true));
            *result.borrow_mut() = Some(
                crate::desktop::notifications::notify("Notes", "Click me", "", Some(on_click))
                    .await,
            );
        });
        super::testing::pump_until("the second notification", Duration::from_secs(5), || {
            delivered.borrow().is_some()
        });
        let Some(Ok(crate::desktop::notifications::Delivered::Server(id))) =
            delivered.borrow().clone()
        else {
            panic!("delivered to the server: {:?}", delivered.borrow())
        };
        // The subscription's match rule reaches the bus asynchronously, so
        // the server keeps reporting the click until it lands.
        let until = Instant::now() + Duration::from_secs(5);
        while !clicked.get() {
            assert!(Instant::now() < until, "the click never arrived");
            server.emit(
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                "ActionInvoked",
                (id, "default").to_variant(),
            );
            super::testing::pump_for(Duration::from_millis(100));
        }
    });
}

#[test]
fn a_url_opens_through_the_openuri_portal() {
    on_gtk(|| {
        let xml = r#"<node><interface name="org.freedesktop.portal.OpenURI">
            <method name="OpenURI"><arg type="s" direction="in"/><arg type="s" direction="in"/>
            <arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method>
            </interface></node>"#;
        let portal = FakeService::start(
            "org.freedesktop.portal.Desktop",
            vec![Object {
                path: "/org/freedesktop/portal/desktop".into(),
                xml: xml.into(),
                handler: Arc::new(|_, _| {
                    Ok((glib::variant::ObjectPath::try_from(
                        "/org/freedesktop/portal/desktop/request/1/t",
                    )
                    .expect("a path"),)
                        .to_variant())
                }),
            }],
        );
        drive(async {
            crate::LinuxSystem::default().open_url("https://example.com/docs".into()).await
        })
        .expect("opened");
        let call = portal.calls_to("OpenURI").first().cloned().expect("the portal was asked");
        let (_, uri, _) = call
            .parameters
            .get::<(String, String, std::collections::HashMap<String, glib::Variant>)>()
            .expect("args");
        assert_eq!(uri, "https://example.com/docs");
    });
}

#[test]
#[allow(
    deprecated,
    reason = "driving GTK's own chooser dialog as a person would, through its FileChooser interface"
)]
fn a_file_dialog_returns_what_the_person_chose_and_none_when_dismissed() {
    on_gtk(|| {
        let chosen = std::env::temp_dir().join("rustnative-dialog-choice.txt");
        std::fs::write(&chosen, b"x").expect("a file to choose");
        for accept in [true, false] {
            let request = FileDialogRequest {
                kind: FileDialogKind::OpenFile,
                title: Some("Pick one".into()),
                filters: vec![("Text".into(), vec!["txt".into()])],
                owner: None,
            };
            let (reply, answer) = mpsc::channel();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime");
                let _ = reply.send(runtime.block_on(crate::LinuxFileDialogs.show(request)));
            });
            let mut dialog = None;
            super::testing::pump_until("GTK's chooser", Duration::from_secs(10), || {
                dialog = gtk::Window::list_toplevels()
                    .into_iter()
                    .find_map(|window| window.downcast::<gtk::FileChooserDialog>().ok())
                    .filter(gtk::prelude::WidgetExt::is_visible);
                dialog.is_some()
            });
            let dialog = dialog.expect("the dialog");
            assert_eq!(dialog.title().as_deref(), Some("Pick one"));
            let response = if accept {
                dialog.set_file(&gtk::gio::File::for_path(&chosen)).expect("selectable");
                // The chooser selects the file once its folder has loaded,
                // and ignores an Accept with nothing selected.
                super::testing::pump_until("the selection", Duration::from_secs(10), || {
                    dialog.file().and_then(|file| file.path()).as_deref() == Some(chosen.as_path())
                });
                gtk::ResponseType::Accept
            } else {
                gtk::ResponseType::Cancel
            };
            dialog.response(response);
            let context = glib::MainContext::default();
            let until = Instant::now() + Duration::from_secs(10);
            let result = loop {
                if let Ok(result) = answer.try_recv() {
                    break result;
                }
                assert!(Instant::now() < until, "the dialog did not answer");
                context.iteration(false);
            };
            let expected = accept.then(|| chosen.to_string_lossy().into_owned());
            assert_eq!(result.expect("no failure"), expected);
        }
    });
}
