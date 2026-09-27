//! The serverless shapes end to end (`PLAN.md` Web milestone K):
//! `examples/web-notes` runs as the long-lived server (SQLite, the
//! indexing job, the data service), as a Lambda function behind
//! `rustnative serve lambda`, and as a WAGI module behind `rustnative serve
//! wagi`. The same flow — sign in, list, add through the server function —
//! works through each, and a session from one shape is good in the others
//! (one sealing key, nothing kept in an instance). The edge emulator stops
//! a route over its fuel budget and a module over its memory ceiling.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::format_push_string,
    clippy::ptr_arg,
    missing_docs,
    reason = "tests"
)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const DATA_KEY: &str = "data-service-key";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn target() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| workspace().join("target"), PathBuf::from)
}

fn cargo(arguments: &[&str]) {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(arguments)
        .current_dir(workspace())
        .status()
        .unwrap();
    assert!(status.success(), "cargo {arguments:?}");
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(port: u16) {
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(60), "nothing listened on {port}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A response: status, headers, body.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

fn send(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response.windows(4).position(|window| window == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&response[..split]).into_owned();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let headers: Vec<(String, String)> = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let mut body = response[split + 4..].to_vec();
    if headers.iter().any(|(name, value)| name == "transfer-encoding" && value.contains("chunked"))
    {
        let mut out = Vec::new();
        let mut rest = &body[..];
        while let Some(end) = rest.windows(2).position(|window| window == b"\r\n") {
            let size = usize::from_str_radix(std::str::from_utf8(&rest[..end]).unwrap().trim(), 16)
                .unwrap();
            if size == 0 {
                break;
            }
            out.extend_from_slice(&rest[end + 2..end + 2 + size]);
            rest = &rest[end + 4 + size..];
        }
        body = out;
    }
    Reply { status, headers, body: String::from_utf8_lossy(&body).into_owned() }
}

/// The cookies a response set, as a `cookie` header.
fn cookies(reply: &Reply, jar: &mut Vec<(String, String)>) -> String {
    for (name, value) in &reply.headers {
        if name == "set-cookie" {
            let pair = value.split(';').next().unwrap();
            let (key, value) = pair.split_once('=').unwrap();
            jar.retain(|(existing, _)| existing != key);
            jar.push((key.to_owned(), value.to_owned()));
        }
    }
    jar.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join("; ")
}

fn csrf(jar: &[(String, String)]) -> String {
    jar.iter().find(|(key, _)| key.ends_with("csrf")).map(|(_, value)| value.clone()).unwrap()
}

/// Signs in as ada through `port`; the cookie header of the session.
fn sign_in(port: u16) -> Vec<(String, String)> {
    let mut jar = Vec::new();
    let page = send(port, "GET", "/", &[], "");
    assert_eq!(page.status, 200, "{}", page.body);
    assert!(page.body.contains("Sign in"), "{}", page.body);
    let cookie = cookies(&page, &mut jar);
    let form = format!("_csrf={}&name=ada&password=analytical+engine", csrf(&jar));
    let signed = send(
        port,
        "POST",
        "/sign-in",
        &[("cookie", &cookie), ("content-type", "application/x-www-form-urlencoded")],
        &form,
    );
    assert_eq!(signed.status, 303, "{}", signed.body);
    assert_eq!(signed.header("location"), Some("/"));
    cookies(&signed, &mut jar);
    jar
}

/// Adds `title` through the server function; the page after.
fn add_and_list(port: u16, jar: &mut Vec<(String, String)>, title: &str) -> String {
    let cookie = jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
    let added = send(
        port,
        "POST",
        "/_fn/add_note",
        &[
            ("cookie", &cookie),
            ("x-csrf-token", &csrf(jar)),
            ("content-type", "application/json"),
            ("accept", "application/json"),
        ],
        &format!("\"{title}\""),
    );
    assert_eq!(added.status, 200, "{}", added.body);
    assert!(added.body.contains(title), "{}", added.body);
    let page = send(port, "GET", "/", &[("cookie", &cookie)], "");
    assert_eq!(page.status, 200, "{}", page.body);
    page.body
}

/// The application running in every shape: the server (with its data
/// service), the Lambda function, and the edge module.
struct Shapes {
    server: u16,
    lambda: u16,
    edge: u16,
    data_url: String,
    module: PathBuf,
    folder: PathBuf,
    processes: Vec<Process>,
}

impl Shapes {
    fn start() -> Self {
        cargo(&["build", "-p", "web-notes", "--bins"]);
        cargo(&[
            "build",
            "-p",
            "web-notes",
            "--bin",
            "web-notes-edge",
            "--release",
            "--target",
            "wasm32-wasip1",
            "--no-default-features",
        ]);
        let exe = std::env::consts::EXE_SUFFIX;
        let bin = target().join("debug");
        let module = target().join("wasm32-wasip1/release/web-notes-edge.wasm");
        let folder = std::env::temp_dir().join(format!(
            "rn-web-notes-{}-{}",
            std::process::id(),
            free_port()
        ));
        std::fs::create_dir_all(&folder).unwrap();

        // The long-lived server, with the data service on.
        let server = free_port();
        let server_process = Process(
            Command::new(bin.join(format!("web-notes{exe}")))
                .env("NOTES_ADDRESS", format!("127.0.0.1:{server}"))
                .env("NOTES_DATABASE", folder.join("notes.db"))
                .env("NOTES_SECRET_KEY", KEY)
                .env("NOTES_DATA_KEY", DATA_KEY)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        wait_for(server);
        let data_url = format!("http://127.0.0.1:{server}");

        // The function, behind the Lambda emulator.
        let lambda = free_port();
        let lambda_process = Process(
            Command::new(env!("CARGO_BIN_EXE_rustnative"))
                .args(["serve", "lambda"])
                .arg(bin.join(format!("web-notes-lambda{exe}")))
                .args(["--address", &format!("127.0.0.1:{lambda}"), "--timeout", "30"])
                .env("NOTES_SECRET_KEY", KEY)
                .env("NOTES_DATA_URL", &data_url)
                .env("NOTES_DATA_KEY", DATA_KEY)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        wait_for(lambda);

        let mut shapes = Self {
            server,
            lambda,
            edge: free_port(),
            data_url,
            module,
            folder,
            processes: vec![server_process, lambda_process],
        };
        // The module, behind the edge emulator.
        let edge = shapes.edge_on(shapes.edge, &[]);
        shapes.processes.push(edge);
        shapes
    }

    /// The edge emulator on `port`, with `extra` arguments.
    fn edge_on(&self, port: u16, extra: &[&str]) -> Process {
        let process = Process(
            Command::new(env!("CARGO_BIN_EXE_rustnative"))
                .args(["serve", "wagi"])
                .arg(&self.module)
                .args(["--address", &format!("127.0.0.1:{port}")])
                .args(["--allow-http", &format!("127.0.0.1:{}", self.server)])
                .args(["--env", &format!("NOTES_SECRET_KEY={KEY}")])
                .args(["--env", &format!("NOTES_DATA_URL={}", self.data_url)])
                .args(["--env", &format!("NOTES_DATA_KEY={DATA_KEY}")])
                .args(extra)
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        wait_for(port);
        process
    }
}

impl Drop for Shapes {
    fn drop(&mut self) {
        self.processes.clear();
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

#[test]
fn one_application_answers_in_every_shape() {
    let shapes = Shapes::start();
    let (server_port, lambda_port, edge_port) = (shapes.server, shapes.lambda, shapes.edge);

    // The same flow through each shape.
    for (shape, port, title) in [
        ("server", server_port, "Milk"),
        ("lambda", lambda_port, "Bread"),
        ("edge", edge_port, "Eggs"),
    ] {
        let mut jar = sign_in(port);
        let page = add_and_list(port, &mut jar, title);
        assert!(page.contains(title), "{shape}: {page}");
        assert!(page.contains("Signed in as ada"), "{shape}: {page}");
    }

    // One session, every shape: nothing is kept in an instance.
    let mut jar = sign_in(lambda_port);
    let cookie = jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
    for port in [server_port, edge_port] {
        let page = send(port, "GET", "/", &[("cookie", &cookie)], "");
        assert!(
            page.body.contains("Milk") && page.body.contains("Bread") && page.body.contains("Eggs"),
            "{}",
            page.body
        );
    }
    // The job indexed what the serverless shapes added, on the server.
    let started = Instant::now();
    loop {
        let page = add_and_list(server_port, &mut jar, "Tea");
        if !page.contains("Eggs (indexing)") && !page.contains("Bread (indexing)") {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "the job never ran: {page}");
        std::thread::sleep(Duration::from_millis(200));
    }

    // The edge host's limits.
    let fuel = send(edge_port, "GET", "/", &[], "");
    let used: u64 = fuel.header("x-rn-fuel").unwrap().parse().unwrap();
    let starved_port = free_port();
    let _starved = shapes.edge_on(starved_port, &["--route-fuel", &format!("/={}", used / 2)]);
    let starved = send(starved_port, "GET", "/", &[], "");
    assert_eq!(starved.status, 503, "{}", starved.body);
    assert_eq!(starved.header("x-rn-limit"), Some("fuel"));
    let cramped_port = free_port();
    let _cramped = shapes.edge_on(cramped_port, &["--memory-mb", "1"]);
    let cramped = send(cramped_port, "GET", "/", &[], "");
    assert_eq!(cramped.status, 503, "{}", cramped.body);
    assert_eq!(cramped.header("x-rn-limit"), Some("memory"));
}

#[test]
fn an_actor_session_serializes_and_persists_on_the_edge() {
    cargo(&[
        "build",
        "-p",
        "edge-actors",
        "--release",
        "--target",
        "wasm32-wasip1",
        "--no-default-features",
    ]);
    let module = target().join("wasm32-wasip1/release/edge-actors.wasm");
    let port = free_port();
    let _edge = Process(
        Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .args(["serve", "wagi"])
            .arg(&module)
            .args(["--address", &format!("127.0.0.1:{port}"), "--actors", "/actors/"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(port);
    let post = |path: &str, body: &str| {
        let reply = send(port, "POST", path, &[("content-type", "application/json")], body);
        assert_eq!(reply.status, 200, "{}", reply.body);
        serde_json::from_str::<serde_json::Value>(&reply.body).unwrap()
    };
    post("/actors/doc-1", r#"{"Join":"ada"}"#);
    post("/actors/doc-1", r#"{"Join":"grace"}"#);
    let writers: Vec<_> = (0..20)
        .map(|index| {
            let who = if index % 2 == 0 { "ada" } else { "grace" };
            let body = format!(r#"{{"Append":["{who}","line {index}"]}}"#);
            std::thread::spawn(move || {
                send(port, "POST", "/actors/doc-1", &[("content-type", "application/json")], &body)
            })
        })
        .collect();
    for writer in writers {
        assert_eq!(writer.join().unwrap().status, 200);
    }
    let read = post("/actors/doc-1", r#""Read""#);
    assert_eq!(read["Ok"]["text"].as_str().unwrap().lines().count(), 20, "no lost update: {read}");
    assert_eq!(read["Ok"]["editors"], serde_json::json!(["ada", "grace"]));
    // The host's scheduler fires the alarm: once.
    assert_eq!(post("/actors/doc-1/alarm", ""), serde_json::json!({ "fired": true }));
    assert_eq!(post("/actors/doc-1/alarm", ""), serde_json::json!({ "fired": false }));
    let again = post("/actors/doc-1", r#""Read""#);
    assert_eq!(
        again["Ok"]["text"], read["Ok"]["text"],
        "every request's instance finds its storage"
    );
    assert_eq!(again["Ok"]["reminders"], 1);
    // Another actor is another document.
    assert_eq!(post("/actors/doc-2", r#""Read""#)["Ok"]["text"], "");
}

/// The page's normalized DOM after each step of the same interactions:
/// loaded, the empty add refused, a title typed.
fn interact(page: &rustnative_web_testing::Page) -> Vec<String> {
    let dom = |page: &rustnative_web_testing::Page| {
        page.eval("document.getElementById('page').outerHTML").unwrap().as_str().unwrap().to_owned()
    };
    if let Err(error) = page.wait_until(
        "document.documentElement.hasAttribute('data-rn-ready')",
        Duration::from_secs(60),
    ) {
        panic!("{error:?}: {:?} {:?}", page.eval("location.href"), page.console());
    }
    let mut steps = vec![dom(page)];
    page.click("#i0-add").unwrap();
    page.wait_until(
        "document.getElementById('i0-error').textContent === 'A note needs a title'",
        Duration::from_secs(20),
    )
    .unwrap();
    steps.push(dom(page));
    page.focus("#i0-draft").unwrap();
    page.type_text("Tea").unwrap();
    page.wait_until("document.getElementById('i0-draft').value === 'Tea'", Duration::from_secs(20))
        .unwrap();
    steps.push(dom(page));
    steps
}

/// Signs in through the form, as a person would (unless the browser's
/// session already holds).
fn browser_sign_in(page: &rustnative_web_testing::Page, origin: &str) {
    page.goto(&format!("{origin}/")).unwrap();
    page.wait_until(
        "!!(document.getElementById('name') || document.getElementById('i0-editor'))",
        Duration::from_secs(60),
    )
    .unwrap();
    // Cookies are per host, not per port: a session from another shape is
    // already good here.
    if page.eval("!!document.getElementById('i0-editor')").unwrap() == serde_json::json!(true) {
        return;
    }
    page.focus("#name").unwrap();
    page.type_text("ada").unwrap();
    page.focus("#password").unwrap();
    page.type_text("analytical engine").unwrap();
    page.click("#go").unwrap();
    if let Err(error) =
        page.wait_until("!!document.getElementById('i0-editor')", Duration::from_secs(60))
    {
        let state =
            page.eval("location.href + ' | ' + document.cookie + ' | ' + document.body.innerHTML");
        panic!("{origin}: {error:?}: {state:?} {:?}", page.console());
    }
}

/// `W-MF-5`: one application, the same state, rendered client-side (the
/// static export), server-rendered, as a function, and on the edge — in
/// Edge, the page's DOM is identical in each, before and after the same
/// interactions. (The same client component as generated JavaScript and as
/// WebAssembly is `rustnative-server/tests/wasm_browser.rs`.)
#[test]
fn the_ui_is_the_same_in_every_mode() {
    let Some(browser) = rustnative_web_testing::Browser::launch().unwrap() else {
        eprintln!("skipped: no Edge or Chrome");
        return;
    };
    let shapes = Shapes::start();
    // The same state everywhere: ada's notes, as the data service has them.
    let mut jar = sign_in(shapes.server);
    add_and_list(shapes.server, &mut jar, "Milk");
    // Once the job has indexed it, so nothing changes while the modes load.
    let started = Instant::now();
    let notes = loop {
        let notes: Vec<web_notes::Note> = serde_json::from_str(
            &send(shapes.server, "GET", "/data/notes?owner=1", &[("x-data-key", DATA_KEY)], "")
                .body,
        )
        .unwrap();
        if notes.iter().all(|note| note.indexed) {
            break notes;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "the job never ran");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(notes.len(), 1);

    // The static export, on a static host.
    let export = shapes.folder.join("export");
    web_notes::site(notes).export(&export).unwrap();
    let static_port = free_port();
    let _static = Process(
        Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .args(["serve", "static"])
            .arg(&export)
            .args(["--address", &format!("127.0.0.1:{static_port}")])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(static_port);

    let mut results = Vec::new();
    for (mode, port, signs_in) in [
        ("client", static_port, false),
        ("server", shapes.server, true),
        ("lambda", shapes.lambda, true),
        ("edge", shapes.edge, true),
    ] {
        let page = browser.page().unwrap();
        let origin = format!("http://localhost:{port}");
        if signs_in {
            browser_sign_in(&page, &origin);
        } else {
            page.goto(&format!("{origin}/")).unwrap();
        }
        results.push((mode, interact(&page)));
    }
    let (first, expected) = &results[0];
    for (mode, steps) in &results[1..] {
        for (index, (step, want)) in steps.iter().zip(expected).enumerate() {
            assert_eq!(step, want, "step {index}: {mode} differs from {first}");
        }
    }
}
