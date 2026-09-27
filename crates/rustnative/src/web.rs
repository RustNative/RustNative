//! The web platform's commands (`PLAN.md` Web milestone J): `build`, `run`,
//! `dev`, and `package` for `web`, and `serve static`, a static host that
//! honours an export's `_headers`.
//!
//! - `rustnative build web --mode client` builds the project's WebAssembly
//!   subtrees (`[web] subtrees` in `rustnative.toml`), then runs the
//!   application with `--export target/web/client`
//!   (`rustnative_web::export::run`), which writes every route, the assets,
//!   `_headers`, the offline files, the sitemap, and the report.
//! - `--mode server` builds the server; `--mode serverless --host lambda`
//!   builds it and stages it as a function runtime's `bootstrap`, and
//!   `--host wagi` builds it for `wasm32-wasip1`.
//! - `rustnative dev web` runs the server behind a development proxy: on
//!   every save it rebuilds and restarts the server, and the open pages
//!   reload with their islands' state kept — or show the build's errors in
//!   an overlay until the next save fixes them.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::project::Project;

/// The `[web]` table of `rustnative.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebConfig {
    /// The crates (workspace packages) built for the browser as WebAssembly
    /// subtrees.
    #[serde(default)]
    pub subtrees: Vec<String>,
    /// The address `run` and `dev` serve on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// The offline application settings the application reads itself
    /// (`rustnative_server::web::pwa_config`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pwa: Option<serde_json::Value>,
}

/// How a web application is deployed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum WebMode {
    /// Static files: every route rendered at build time.
    Client,
    /// A long-running server.
    Server,
    /// A function per request (`--host lambda` or `--host wagi`).
    Serverless,
}

/// Where a serverless build runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum WebHost {
    /// A function runtime speaking AWS Lambda's runtime API.
    Lambda,
    /// A WebAssembly host speaking WAGI (CGI over WASI), as Spin runs it.
    Wagi,
}

fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> Error {
    let what = what.into();
    move |cause| Error::Io { what, cause }
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root);
    command
}

fn checked(mut command: Command) -> Result<()> {
    let status = command.status().map_err(|cause| Error::ToolMissing {
        tool: "cargo",
        hint: "install Rust from https://rustup.rs".into(),
        cause: Some(cause.to_string()),
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::ToolFailed { tool: "cargo", code: status.code() })
    }
}

fn web_config(project: &Project) -> WebConfig {
    project.config.web.clone().unwrap_or_default()
}

/// Builds the project's WebAssembly subtrees into `target/web/wasm`; their
/// files, by module name.
///
/// # Errors
///
/// A subtree did not build.
pub fn build_subtrees(project: &Project) -> Result<Vec<(String, PathBuf)>> {
    let target = crate::dev::target_dir(&project.root);
    let staged = target.join("web/wasm");
    std::fs::create_dir_all(&staged).map_err(io("create target/web/wasm"))?;
    let mut built = Vec::new();
    for subtree in &web_config(project).subtrees {
        println!("build: the {subtree} subtree for wasm32-unknown-unknown");
        let mut command = cargo(&project.root);
        command.args(["build", "--release", "--target", "wasm32-unknown-unknown", "-p", subtree]);
        checked(command)?;
        let file = target
            .join("wasm32-unknown-unknown/release")
            .join(format!("{}.wasm", subtree.replace('-', "_")));
        let into = staged.join(format!("{subtree}.wasm"));
        std::fs::copy(&file, &into).map_err(io(format!("copy {}", file.display())))?;
        built.push((subtree.clone(), into));
    }
    Ok(built)
}

/// `rustnative build web`: what it built.
///
/// # Errors
///
/// A build step failed.
pub fn build(
    project: &Project,
    mode: WebMode,
    host: Option<WebHost>,
    release: bool,
) -> Result<PathBuf> {
    let subtrees = build_subtrees(project)?;
    let target = crate::dev::target_dir(&project.root);
    let profile = if release { "release" } else { "debug" };
    let name = &project.config.app.name;
    match (mode, host) {
        (WebMode::Client, _) => {
            let folder = target.join("web/client");
            let _ = std::fs::remove_dir_all(&folder);
            let mut command = cargo(&project.root);
            command.arg("run");
            if release {
                command.arg("--release");
            }
            command.args(["--", "--export"]).arg(&folder);
            command.env("RUSTNATIVE_WASM_DIR", target.join("web/wasm"));
            checked(command)?;
            for (module, file) in &subtrees {
                let into = folder.join("_rn/w").join(format!("{module}.wasm"));
                std::fs::create_dir_all(folder.join("_rn/w")).map_err(io("create _rn/w"))?;
                std::fs::copy(file, &into).map_err(io(format!("copy {}", file.display())))?;
            }
            println!("build: the static site is in {}", folder.display());
            Ok(folder)
        }
        (WebMode::Server, _) | (WebMode::Serverless, Some(WebHost::Lambda)) => {
            let mut command = cargo(&project.root);
            command.arg("build");
            if release {
                command.arg("--release");
            }
            checked(command)?;
            let executable =
                target.join(profile).join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            if mode == WebMode::Server {
                println!("build: the server is {}", executable.display());
                return Ok(executable);
            }
            // A function runtime starts `bootstrap`.
            let folder = target.join("web/lambda");
            std::fs::create_dir_all(&folder).map_err(io("create target/web/lambda"))?;
            let bootstrap = folder.join(format!("bootstrap{}", std::env::consts::EXE_SUFFIX));
            std::fs::copy(&executable, &bootstrap).map_err(io("stage bootstrap"))?;
            println!("build: the function is {}", bootstrap.display());
            Ok(bootstrap)
        }
        (WebMode::Serverless, host) => {
            if host.is_none() {
                return Err(Error::Usage(
                    "serverless builds name their host: --host lambda or --host wagi".into(),
                ));
            }
            let mut command = cargo(&project.root);
            command.args(["build", "--release", "--target", "wasm32-wasip1"]);
            checked(command)?;
            let module = target.join("wasm32-wasip1/release").join(format!("{name}.wasm"));
            let folder = target.join("web/wagi");
            std::fs::create_dir_all(&folder).map_err(io("create target/web/wagi"))?;
            let staged = folder.join(format!("{name}.wasm"));
            std::fs::copy(&module, &staged).map_err(io(format!("copy {}", module.display())))?;
            println!("build: the WAGI module is {}", staged.display());
            Ok(staged)
        }
    }
}

/// The address `run` and `dev` serve on.
fn address(project: &Project) -> String {
    web_config(project).address.unwrap_or_else(|| "127.0.0.1:3000".to_owned())
}

/// `rustnative run web`.
///
/// # Errors
///
/// The build or the server failed.
pub fn run(project: &Project, release: bool) -> Result<()> {
    build_subtrees(project)?;
    let target = crate::dev::target_dir(&project.root);
    let mut command = cargo(&project.root);
    command.arg("run");
    if release {
        command.arg("--release");
    }
    command
        .env("RUSTNATIVE_WEB_ADDR", address(project))
        .env("RUSTNATIVE_WASM_DIR", target.join("web/wasm"));
    println!("run: http://{}", address(project));
    checked(command)
}

/// `rustnative package web`: the server, built for release, with its
/// WebAssembly modules, as one archive.
///
/// # Errors
///
/// The build failed or the archive could not be written.
pub fn package(project: &Project) -> Result<PathBuf> {
    let executable = build(project, WebMode::Server, None, true)?;
    let target = crate::dev::target_dir(&project.root);
    let mut entries = vec![
        crate::package::zip::entry_from(
            &executable,
            &executable
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
        .map_err(io("read the server"))?,
    ];
    for (module, file) in build_subtrees(project)? {
        entries.push(
            crate::package::zip::entry_from(&file, &format!("wasm/{module}.wasm"))
                .map_err(io("read a module"))?,
        );
    }
    let folder = target.join("package/web");
    std::fs::create_dir_all(&folder).map_err(io("create target/package/web"))?;
    let path =
        folder.join(format!("{}-{}.zip", project.config.app.name, project.config.app.version));
    std::fs::write(&path, crate::package::zip::archive(entries))
        .map_err(io("write the archive"))?;
    println!("package: {}", path.display());
    Ok(path)
}

// ------------------------------------------------------ the static host ----

/// The headers an export's `_headers` gives `path`: every block whose
/// pattern matches, in order, `! Name` removing an earlier header.
#[must_use]
pub fn headers_for(rules: &str, path: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut applies = false;
    for line in rules.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            let pattern = line.trim();
            applies = pattern == path
                || pattern.strip_suffix('*').is_some_and(|prefix| path.starts_with(prefix));
            continue;
        }
        if !applies {
            continue;
        }
        let line = line.trim();
        if let Some(name) = line.strip_prefix('!') {
            let name = name.trim().to_ascii_lowercase();
            headers.retain(|(existing, _)| *existing != name);
        } else if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            headers.retain(|(existing, _)| *existing != name);
            headers.push((name, value.trim().to_owned()));
        }
    }
    headers
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("wasm") => "application/wasm",
        Some("webp") => "image/webp",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ttf") => "font/ttf",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

/// Answers one request against the export in `folder`: the file (a folder
/// is its `index.html`), with the headers `_headers` gives it.
fn answer_static(folder: &Path, rules: &str, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let clean = path.split(['?', '#']).next().unwrap_or("/");
    if clean.split('/').any(|part| part == "..") {
        return (400, Vec::new(), b"bad path".to_vec());
    }
    let mut file = folder.join(clean.trim_start_matches('/'));
    if file.is_dir() {
        file = file.join("index.html");
    }
    let Ok(body) = std::fs::read(&file) else {
        return (404, vec![("content-type".into(), "text/plain".into())], b"not found".to_vec());
    };
    let mut headers = vec![("content-type".to_owned(), content_type(&file).to_owned())];
    for (name, value) in headers_for(rules, clean) {
        headers.retain(|(existing, _)| *existing != name);
        headers.push((name, value));
    }
    (200, headers, body)
}

fn read_request_head(stream: &mut TcpStream) -> Option<(Vec<u8>, String, String)> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 || head.len() > 64 * 1024 {
            return None;
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut parts = text.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();
    Some((head, method, path))
}

fn write_response(stream: &mut TcpStream, status: u16, headers: &[(String, String)], body: &[u8]) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Bad Request",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// `rustnative serve static <folder>`: serves an export as a static host
/// does, `_headers` included, until the process is stopped (or, with
/// `requests`, after that many requests).
///
/// # Errors
///
/// The address could not be bound.
pub fn serve_static(folder: &Path, address: SocketAddr, requests: Option<usize>) -> Result<()> {
    let listener = TcpListener::bind(address).map_err(io(format!("listen on {address}")))?;
    println!("serve: http://{}", listener.local_addr().unwrap_or(address));
    serve_static_on(&listener, folder, requests);
    Ok(())
}

/// [`serve_static`] on an existing listener.
pub fn serve_static_on(listener: &TcpListener, folder: &Path, requests: Option<usize>) {
    let rules = std::fs::read_to_string(folder.join("_headers")).unwrap_or_default();
    let mut served = 0;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if let Some((_, _, path)) = read_request_head(&mut stream) {
            let (status, headers, body) = answer_static(folder, &rules, &path);
            write_response(&mut stream, status, &headers, &body);
        }
        served += 1;
        if requests.is_some_and(|limit| served >= limit) {
            return;
        }
    }
}

// ------------------------------------------------------ the dev loop ----

/// What the development proxy tells the open pages.
#[derive(Default)]
struct DevState {
    /// Where the server listens, while it runs.
    backend: Option<SocketAddr>,
    /// The last build's errors, while it is broken.
    error: Option<String>,
    /// The pages listening for news (`/_rn/dev`).
    listeners: Vec<TcpStream>,
}

fn announce(state: &Mutex<DevState>, event: &str, data: &str) {
    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
    let message = format!("event: {event}\ndata: {}\n\n", data.replace('\n', "\ndata: "));
    state.listeners.retain_mut(|stream| stream.write_all(message.as_bytes()).is_ok());
}

fn proxy(mut client: TcpStream, state: &Arc<Mutex<DevState>>) {
    let Some((head, _, path)) = read_request_head(&mut client) else { return };
    if path.starts_with("/_rn/dev") {
        let _ = client.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\r\n");
        let error = state.lock().unwrap_or_else(PoisonError::into_inner).error.clone();
        let _ = client.write_all(b"event: ready\ndata: \n\n");
        if let Some(error) = error {
            let _ = client.write_all(
                format!("event: error\ndata: {}\n\n", error.replace('\n', "\ndata: ")).as_bytes(),
            );
        }
        state.lock().unwrap_or_else(PoisonError::into_inner).listeners.push(client);
        return;
    }
    let backend = state.lock().unwrap_or_else(PoisonError::into_inner).backend;
    let Some(backend) = backend.and_then(|address| TcpStream::connect(address).ok()) else {
        let error = state.lock().unwrap_or_else(PoisonError::into_inner).error.clone();
        let text = error.unwrap_or_else(|| "The server is starting…".to_owned());
        let body = format!(
            "<!doctype html><title>Building</title><pre>{}</pre><p>This page reloads when the server is back.</p>",
            text.replace('&', "&amp;").replace('<', "&lt;")
        );
        write_response(
            &mut client,
            503,
            &[
                ("content-type".into(), "text/html; charset=utf-8".into()),
                ("refresh".into(), "1".into()),
            ],
            body.as_bytes(),
        );
        return;
    };
    let mut upstream = backend;
    if upstream.write_all(&head).is_err() {
        return;
    }
    let (Ok(mut client_reader), Ok(mut upstream_writer)) =
        (client.try_clone(), upstream.try_clone())
    else {
        return;
    };
    let forward = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_reader, &mut upstream_writer);
        let _ = upstream_writer.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut upstream, &mut client);
    let _ = client.shutdown(std::net::Shutdown::Both);
    let _ = forward.join();
}

fn free_address() -> Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(io("find a free port"))?;
    listener.local_addr().map_err(io("find a free port"))
}

fn start_server(project: &Project, backend: SocketAddr) -> Result<Child> {
    let target = crate::dev::target_dir(&project.root);
    let executable = target.join("debug").join(format!(
        "{}{}",
        project.config.app.name,
        std::env::consts::EXE_SUFFIX
    ));
    Command::new(&executable)
        .current_dir(&project.root)
        .env("RUSTNATIVE_WEB_ADDR", backend.to_string())
        .env("RUSTNATIVE_DEV", "1")
        .env("RUSTNATIVE_WASM_DIR", target.join("web/wasm"))
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(io(format!("start {}", executable.display())))
}

fn build_for_dev(project: &Project) -> std::result::Result<(), String> {
    build_subtrees(project).map_err(|error| error.to_string())?;
    let output = cargo(&project.root)
        .args(["build", "--message-format", "short"])
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        Err(text
            .lines()
            .filter(|line| line.contains("error") || line.contains("-->"))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

fn wait_listening(address: SocketAddr) {
    for _ in 0..200 {
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `rustnative dev web`: the server behind a development proxy at the
/// project's address, rebuilt and restarted on every save; open pages
/// reload with their islands' state, or show the build's errors.
///
/// # Errors
///
/// The proxy's address could not be bound.
pub fn dev(project: &Project, once: bool) -> Result<()> {
    let public: SocketAddr = address(project).parse().map_err(|_| {
        Error::Usage(format!("[web] address {:?} is not an address", address(project)))
    })?;
    let listener = TcpListener::bind(public).map_err(io(format!("listen on {public}")))?;
    let state = Arc::new(Mutex::new(DevState::default()));
    {
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let state = Arc::clone(&state);
                std::thread::spawn(move || proxy(client, &state));
            }
        });
    }
    println!("dev: http://{public}");
    let mut server: Option<Child> = None;
    let mut files = crate::dev::scan(&project.root);
    loop {
        match build_for_dev(project) {
            Ok(()) => {
                if let Some(mut old) = server.take() {
                    let _ = old.kill();
                    let _ = old.wait();
                }
                let backend = free_address()?;
                server = Some(start_server(project, backend)?);
                wait_listening(backend);
                {
                    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                    state.backend = Some(backend);
                    state.error = None;
                }
                announce(&state, "reload", "");
                println!("dev: running");
            }
            Err(error) => {
                println!("dev: the build failed\n{error}");
                state.lock().unwrap_or_else(PoisonError::into_inner).error = Some(error.clone());
                announce(&state, "error", &error);
            }
        }
        if once {
            if let Some(mut server) = server {
                let _ = server.kill();
            }
            return Ok(());
        }
        loop {
            std::thread::sleep(Duration::from_millis(300));
            let now = crate::dev::scan(&project.root);
            if !crate::dev::changed(&files, &now).is_empty() {
                files = now;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "/*\n  Content-Security-Policy: default-src 'self'\n  X-Frame-Options: DENY\n\n/_rn/*\n  Cache-Control: public, max-age=31536000, immutable\n\n/_rn/w/*\n  ! Cache-Control\n  Cache-Control: no-cache\n\n/game\n  ! Content-Security-Policy\n  Content-Security-Policy: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'\n";

    #[test]
    fn headers_follow_the_rules_in_order() {
        let page = headers_for(RULES, "/about");
        assert!(page.contains(&("x-frame-options".into(), "DENY".into())));
        assert!(!page.iter().any(|(name, _)| name == "cache-control"));
        let asset = headers_for(RULES, "/_rn/rn.abc.js");
        assert!(
            asset.contains(&("cache-control".into(), "public, max-age=31536000, immutable".into()))
        );
        let module = headers_for(RULES, "/_rn/w/app.wasm");
        assert_eq!(module.iter().filter(|(name, _)| name == "cache-control").count(), 1);
        assert!(module.contains(&("cache-control".into(), "no-cache".into())));
        let game = headers_for(RULES, "/game");
        assert!(
            game.iter().any(|(name, value)| name == "content-security-policy"
                && value.contains("wasm-unsafe-eval"))
        );
    }

    #[test]
    fn the_static_host_serves_an_export_with_its_headers() {
        let folder = std::env::temp_dir().join(format!("rn-static-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("about")).unwrap();
        std::fs::write(folder.join("index.html"), "<p>home</p>").unwrap();
        std::fs::write(folder.join("about/index.html"), "<p>about</p>").unwrap();
        std::fs::write(folder.join("_headers"), RULES).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let root = folder.clone();
        let server = std::thread::spawn(move || serve_static_on(&listener, &root, Some(3)));
        let get = |path: &str| {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).unwrap();
            let mut text = String::new();
            stream.read_to_string(&mut text).unwrap();
            text
        };
        let about = get("/about");
        assert!(about.starts_with("HTTP/1.1 200") && about.contains("<p>about</p>"), "{about}");
        assert!(about.contains("content-security-policy: default-src 'self'"), "{about}");
        assert!(get("/../secret").starts_with("HTTP/1.1 400"));
        assert!(get("/missing").starts_with("HTTP/1.1 404"));
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&folder);
    }
}
