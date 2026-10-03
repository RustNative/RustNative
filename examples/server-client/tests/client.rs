//! The notes client against the notes server (`PLAN.md` Milestone 49's
//! "done when"): the client signs in, writes a note, and shows the server's
//! answer and its server-rendered summary — in process on the headless
//! backend, and over a real socket through the host's own HTTP stack.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use notes_shared::{CreateNote, Credentials, ListNotes, NewNote, SignIn};
use rustnative_core::server_fn::call;
use rustnative_core::{Component, Services, Size, Theme, Window};
use rustnative_headless::{HeadlessApp, Query};
use rustnative_server::db::Db;
use rustnative_server::jobs::Jobs;
use rustnative_server::local::InProcess;
use server_client::{Connection, NotesClient};
use server_demo::{IndexNote, app, database};

fn server(name: &str) -> (rustnative_server::AppService, Jobs) {
    let db = Db::memory(name, 2).unwrap();
    database(&db).unwrap();
    let jobs = Jobs::new(db.clone()).unwrap().register::<IndexNote>();
    (app(&db, &jobs, [6; 32]).into_service(), jobs)
}

fn text(app: &HeadlessApp, key: &str) -> String {
    app.find(&Query::key(key)).map(|node| node.text.clone().unwrap_or_default()).unwrap_or_default()
}

/// Settles until `done` holds: the server answers on the runtime's
/// threads, not the headless executor's.
fn settle_until(app: &mut HeadlessApp, done: impl Fn(&HeadlessApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out:\n{}", app.golden());
        app.settle();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_client_signs_in_writes_and_shows_the_servers_answer() {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let _inside = runtime.enter();
    let (service, jobs) = server("client-flow");
    let services = Services::default().with_http(Arc::new(InProcess(service)));
    let connection = Connection {
        base: "https://notes.example.com".into(),
        name: "grace".into(),
        password: "compiler".into(),
    };
    let mut app = HeadlessApp::launch_with(
        Window::new("Notes", Size::new(520, 600)),
        services,
        Theme::default(),
        move || NotesClient::new(connection.clone()),
    );
    settle_until(&mut app, |app| text(app, "status") == "Signed in as grace");
    assert_eq!(text(&app, "heading"), "grace's notes", "the shared view");

    app.set_text(&Query::key("draft"), "Water the plants").unwrap();
    app.click(&Query::key("add")).unwrap();
    settle_until(&mut app, |app| text(app, "summary") == "1 notes, 0 indexed");
    assert!(text(&app, "note-1").starts_with("Water the plants"));

    runtime.block_on(jobs.work_until_idle());
    app.set_text(&Query::key("draft"), "Call Ada").unwrap();
    app.click(&Query::key("add")).unwrap();
    settle_until(&mut app, |app| text(app, "summary") == "2 notes, 1 indexed");
    assert_eq!(text(&app, "note-1"), "Water the plants", "indexed by the server's job");
}

/// The host's own HTTP stack (Windows' `WinHttp`, libsoup on Linux) calls
/// the typed functions of a server listening on a real socket.
#[cfg(any(windows, target_os = "linux"))]
#[test]
fn the_host_http_stack_calls_the_server_over_a_socket() {
    #[cfg(target_os = "linux")]
    use rustnative_linux::SoupHttp as Http;
    #[cfg(windows)]
    use rustnative_windows::WinHttp as Http;

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let (service, _) = {
        let _inside = runtime.enter();
        server("client-socket")
    };
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    runtime.spawn(service.serve(listener, std::future::pending()));

    let http = Http::new();
    let result = runtime.block_on(async {
        let token = call::<SignIn>(
            &http,
            &base,
            &Credentials { name: "ada".into(), password: "analytical engine".into() },
            &[],
        )
        .await?;
        let bearer = format!("Bearer {token}");
        let auth = [("authorization", bearer.as_str())];
        call::<CreateNote>(&http, &base, &NewNote { title: "From the host".into() }, &auth).await?;
        call::<ListNotes>(&http, &base, &(), &auth).await
    });
    let notes = result.unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].title, "From the host");
}
