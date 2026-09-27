//! A new web application, end to end (`PLAN.md` Web milestone J's
//! development loop, checked on Web milestone K's template): `rustnative
//! new --web` makes a project whose test passes; `rustnative dev web`
//! serves it; in Edge, the island counts; an edit rebuilds and reloads the
//! page with the island's state kept; a broken build shows its errors over
//! the page, and the fix takes them away.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, reason = "tests")]

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        // Stopping `dev` from outside leaves the server it started running
        // (Windows does not stop a process's children with it).
        if cfg!(windows) {
            let _ = Command::new("taskkill")
                .args(["/F", "/IM", "hello-web.exe"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[test]
fn a_new_web_application_develops_in_the_browser() {
    let Some(browser) = rustnative_web_testing::Browser::launch().unwrap() else {
        eprintln!("skipped: no Edge or Chrome");
        return;
    };
    let parent = std::env::temp_dir().join(format!("rn-new-web-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    // The framework's own target folder, so its dependencies are built once.
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace().join("target"), PathBuf::from);
    let status = Command::new(env!("CARGO_BIN_EXE_rustnative"))
        .current_dir(&parent)
        .args(["new", "hello-web", "--web", "--framework-path"])
        .arg(workspace())
        .status()
        .unwrap();
    assert!(status.success());
    let project = parent.join("hello-web");

    // Its test passes as generated.
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(&project)
        .arg("test")
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .unwrap();
    assert!(status.success(), "the generated project's test");

    // `dev web` on a free port.
    let port = free_port();
    let config = project.join("rustnative.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, text.replace("127.0.0.1:3000", &format!("127.0.0.1:{port}"))).unwrap();
    let _dev = Process(
        Command::new(env!("CARGO_BIN_EXE_rustnative"))
            .current_dir(&project)
            .args(["dev", "web"])
            .env("CARGO_TARGET_DIR", &target)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(started.elapsed() < Duration::from_secs(60), "dev never listened");
        std::thread::sleep(Duration::from_millis(100));
    }

    let page = browser.page().unwrap();
    // The proxy answers "building" (and refreshes) until the server is up.
    page.goto(&format!("http://localhost:{port}/")).unwrap();
    page.wait_until(
        "document.documentElement.hasAttribute('data-rn-ready')",
        Duration::from_secs(600),
    )
    .unwrap();
    page.click("#i0-click").unwrap();
    page.wait_until(
        "document.getElementById('i0-count').textContent === 'Clicked 1 times'",
        Duration::from_secs(20),
    )
    .unwrap();

    // An edit: rebuilt, restarted, and the page reloads, keeping the count.
    let lib = project.join("src/lib.rs");
    let source = std::fs::read_to_string(&lib).unwrap();
    std::fs::write(&lib, source.replace("\"Hello from ", "\"Welcome to ")).unwrap();
    page.wait_until(
        "(document.getElementById('greeting') || {}).textContent === 'Welcome to hello-web'",
        Duration::from_secs(600),
    )
    .unwrap();
    page.wait_until(
        "document.documentElement.hasAttribute('data-rn-ready') && document.getElementById('i0-count').textContent === 'Clicked 1 times'",
        Duration::from_secs(20),
    )
    .unwrap();

    // A broken build: its errors over the page.
    let edited = std::fs::read_to_string(&lib).unwrap();
    std::fs::write(&lib, edited.replace("pub struct Home;", "pub struct Home")).unwrap();
    page.wait_until(
        "(document.getElementById('rn-dev-overlay') || {}).textContent?.includes('The build failed')",
        Duration::from_secs(600),
    )
    .unwrap();
    // Fixed: the page reloads without them.
    std::fs::write(&lib, edited).unwrap();
    page.wait_until(
        "!document.getElementById('rn-dev-overlay') && document.documentElement.hasAttribute('data-rn-ready')",
        Duration::from_secs(600),
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(&parent);
}
