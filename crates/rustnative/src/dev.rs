//! `rustnative dev`: the development loop (`PLAN.md` Milestone 43).
//!
//! The loop builds the application and runs it with inspection on
//! (`RUSTNATIVE_INSPECT`) and in development mode (`RUSTNATIVE_DEV`). It then
//! watches the project and handles each save one of two ways, and says which:
//!
//! - **A token-only `app.css` change** (the values of `@theme` tokens) goes
//!   to the running application, which re-resolves its style. Tokens are
//!   references, not constants (Milestone 58), so nothing is rebuilt.
//! - **Anything else** is rebuilt: Rust, `.rsx`, a utility added or
//!   removed, `Cargo.toml`, `build.rs`, `rustnative.toml`. (A change to
//!   `locales/*.ftl` alone replaces those catalogues in the running
//!   application instead, through inspection's `SetCatalogue`.) If the build
//!   fails, the running application stays and the errors are shown. If it
//!   succeeds:
//!   1. every inspectable component's state is read from the running
//!      application;
//!   2. the application is closed as a person would close it, so persisted
//!      state is flushed and the window's placement saved;
//!   3. the new build is started;
//!   4. the state is written back through each component's
//!      `Component::edit`.
//!
//!   The developer stays where they were. How long this took is printed.
//!
//! With `--remote <addr> --token <token>` the application runs on another
//! machine, under `rustnative dev-agent`: each build is sent to the agent,
//! which starts it; the state is read and restored over the same
//! inspection protocol.
//!
//! The declared development resources (`[resources]`) are provisioned under
//! `target/rustnative-dev/resources` when absent, and passed to the
//! application.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use rustnative_core::inspect::{Endpoint, Reply, Request, endpoint_directory, send_request};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::ResourceKind;
use crate::error::{Error, Result};
use crate::project::Project;

/// How long a started application has to publish its inspection endpoint.
const START_TIMEOUT: Duration = Duration::from_secs(60);
/// How often the project is looked at.
const POLL: Duration = Duration::from_millis(250);

/// Where the application runs.
#[derive(Debug, Clone)]
pub struct Remote {
    /// The agent's address.
    pub addr: SocketAddr,
    /// The token the agent printed.
    pub token: String,
}

/// What one save needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Only `@theme` token values changed: re-resolve, no rebuild.
    Theme,
    /// Only catalogue files (`locales/*.ftl`) changed: replace them in the
    /// running application, no rebuild (Milestone 46).
    Catalogues,
    /// Anything else: rebuild and restart.
    Rebuild,
}

/// `app.css`'s shape: everything that decides what the build generates.
/// Token values are left out of it, unless the theme is `inline`, whose
/// values are folded in at build time.
fn shape(css: &str) -> Option<Vec<String>> {
    let sheet = rustnative_style::sheet::parse(css).ok()?;
    Some(
        sheet
            .items
            .iter()
            .map(|item| match item {
                rustnative_style::sheet::Item::Theme { inline: false, tokens } => {
                    let names: Vec<&str> = tokens.iter().map(|token| token.name.as_str()).collect();
                    format!("theme {names:?}")
                }
                other => format!("{other:?}"),
            })
            .collect(),
    )
}

/// Whether going from `old` to `new` changes only token values.
#[must_use]
pub fn token_only(old: &str, new: &str) -> bool {
    old != new && matches!((shape(old), shape(new)), (Some(old), Some(new)) if old == new)
}

/// What the saves in `changed` need, given `app.css` before and after.
#[must_use]
pub fn classify(changed: &[PathBuf], old_css: Option<&str>, new_css: Option<&str>) -> Change {
    let catalogue = |path: &PathBuf| {
        path.extension().is_some_and(|extension| extension == "ftl")
            && path.parent().and_then(Path::file_name).is_some_and(|folder| folder == "locales")
    };
    if !changed.is_empty() && changed.iter().all(catalogue) {
        return Change::Catalogues;
    }
    let only_css = !changed.is_empty()
        && changed.iter().all(|path| path.file_name().is_some_and(|name| name == "app.css"));
    match (only_css, old_css, new_css) {
        (true, Some(old), Some(new)) if token_only(old, new) => Change::Theme,
        _ => Change::Rebuild,
    }
}

/// The files the loop watches, with when each last changed.
fn walk(directory: &Path, into: &mut BTreeMap<PathBuf, SystemTime>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, into);
        } else if let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) {
            into.insert(path, modified);
        }
    }
}

pub(crate) fn scan(root: &Path) -> BTreeMap<PathBuf, SystemTime> {
    let mut files = BTreeMap::new();
    for folder in ["src", "locales"] {
        walk(&root.join(folder), &mut files);
    }
    for file in ["app.css", "Cargo.toml", "build.rs", crate::config::FILE_NAME] {
        let path = root.join(file);
        if let Ok(modified) = std::fs::metadata(&path).and_then(|meta| meta.modified()) {
            files.insert(path, modified);
        }
    }
    files
}

pub(crate) fn changed(
    before: &BTreeMap<PathBuf, SystemTime>,
    after: &BTreeMap<PathBuf, SystemTime>,
) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = after
        .iter()
        .filter(|(path, modified)| before.get(*path) != Some(*modified))
        .map(|(path, _)| path.clone())
        .collect();
    paths.extend(before.keys().filter(|path| !after.contains_key(*path)).cloned());
    paths
}

/// Where Cargo builds the project: `cargo metadata`'s answer, which knows
/// about workspaces and `CARGO_TARGET_DIR`.
pub(crate) fn target_dir(root: &Path) -> PathBuf {
    let metadata = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .ok()
        .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok());
    metadata
        .and_then(|metadata| metadata.get("target_directory")?.as_str().map(PathBuf::from))
        .unwrap_or_else(|| root.join("target"))
}

fn executable(project: &Project) -> PathBuf {
    target_dir(&project.root).join("debug").join(format!(
        "{}{}",
        project.config.app.name,
        std::env::consts::EXE_SUFFIX
    ))
}

/// The resources the project declares, provisioned: `(name, path)`.
fn provision(project: &Project) -> Result<Vec<(String, PathBuf)>> {
    let base = project.root.join("target").join("rustnative-dev").join("resources");
    let mut provisioned = Vec::new();
    for (name, resource) in &project.config.resources {
        let path = base.join(name);
        if !path.exists() {
            let io = |cause| Error::Io { what: format!("provision resource `{name}`"), cause };
            match (resource.kind, &resource.seed) {
                (ResourceKind::Directory, None) => std::fs::create_dir_all(&path).map_err(io)?,
                (ResourceKind::Directory, Some(seed)) => {
                    copy_tree(&project.root.join(seed), &path).map_err(io)?;
                }
                (ResourceKind::File, seed) => {
                    std::fs::create_dir_all(&base).map_err(io)?;
                    match seed {
                        Some(seed) => std::fs::copy(project.root.join(seed), &path).map(|_| ()),
                        None => std::fs::write(&path, []),
                    }
                    .map_err(io)?;
                }
            }
            println!("dev: provisioned resource `{name}` at {}", path.display());
        }
        provisioned.push((name.clone(), path));
    }
    Ok(provisioned)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn resource_env(resources: &[(String, PathBuf)]) -> Vec<(String, String)> {
    resources
        .iter()
        .map(|(name, path)| {
            let variable =
                format!("RUSTNATIVE_RESOURCE_{}", name.to_ascii_uppercase().replace('-', "_"));
            (variable, path.display().to_string())
        })
        .collect()
}

/// Waits for process `pid` to publish its inspection endpoint.
fn wait_for_endpoint(pid: u32, mut exited: impl FnMut() -> Option<String>) -> Result<Endpoint> {
    let file = endpoint_directory().join(format!("{pid}.json"));
    let started = Instant::now();
    loop {
        if let Some(endpoint) =
            std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            return Ok(endpoint);
        }
        if let Some(status) = exited() {
            return Err(Error::Usage(format!(
                "the application exited before it started ({status})"
            )));
        }
        if started.elapsed() > START_TIMEOUT {
            return Err(Error::Usage(format!(
                "the application did not start within {}s",
                START_TIMEOUT.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn ask(endpoint: &Endpoint, request: &Request) -> Result<Value> {
    match send_request(endpoint, request)
        .map_err(|cause| Error::Io { what: "talk to the running application".into(), cause })?
    {
        Reply::Ok(value) => Ok(value),
        Reply::Error(message) => Err(Error::Usage(format!("the application says: {message}"))),
    }
}

/// Every inspectable component's state, by key path.
fn snapshot(endpoint: &Endpoint) -> BTreeMap<String, Value> {
    ask(endpoint, &Request::Components { window: None })
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|component| {
            let state = component.get("state").filter(|state| !state.is_null())?.clone();
            Some((component.get("path")?.as_str()?.to_owned(), state))
        })
        .collect()
}

/// Writes `states` back, field by field. Returns `(restored, refused)`.
fn restore(endpoint: &Endpoint, states: &BTreeMap<String, Value>) -> (usize, usize) {
    let (mut restored, mut refused) = (0, 0);
    for (path, state) in states {
        let Some(fields) = state.as_object() else { continue };
        for (field, value) in fields {
            let request = Request::SetState {
                path: path.clone(),
                field: field.clone(),
                value: value.clone(),
                window: None,
            };
            if ask(endpoint, &request).is_ok() { restored += 1 } else { refused += 1 }
        }
    }
    (restored, refused)
}

/// Where the application runs, and how to start and stop it there.
enum Host {
    Local { child: Option<Child>, resources: Vec<(String, String)> },
    Remote(Remote),
}

impl Host {
    fn start(&mut self, executable: &Path) -> Result<Endpoint> {
        match self {
            Self::Local { child, resources } => {
                // A copy runs, so the next build can replace the executable
                // (Windows will not overwrite a running one).
                let running = executable
                    .parent()
                    .and_then(Path::parent)
                    .map_or_else(PathBuf::new, Path::to_path_buf)
                    .join("rustnative-dev")
                    .join(executable.file_name().unwrap_or_default());
                std::fs::create_dir_all(running.parent().unwrap_or(Path::new(".")))
                    .and_then(|()| std::fs::copy(executable, &running))
                    .map_err(|cause| Error::Io {
                        what: format!("copy {} to run it", executable.display()),
                        cause,
                    })?;
                let mut process = Command::new(&running)
                    .env(rustnative_core::inspect::INSPECT_VARIABLE, "1")
                    .env(rustnative_core::dev::DEV_VARIABLE, "1")
                    .envs(resources.iter().map(|(key, value)| (key.as_str(), value.as_str())))
                    .spawn()
                    .map_err(|cause| Error::Io {
                        what: format!("start {}", running.display()),
                        cause,
                    })?;
                let pid = process.id();
                let endpoint = wait_for_endpoint(pid, || {
                    process.try_wait().ok().flatten().map(|status| status.to_string())
                });
                *child = Some(process);
                endpoint
            }
            Self::Remote(remote) => {
                let reply = deploy(remote, executable, &[])?;
                reply.endpoint.ok_or_else(|| {
                    Error::Usage(format!(
                        "the application on {} exited before it started ({})",
                        remote.addr,
                        reply
                            .exited
                            .map_or_else(|| "no status".to_owned(), |code| code.to_string())
                    ))
                })
            }
        }
    }

    /// Asks the application to close; a local one that does not is ended.
    fn stop(&mut self, endpoint: &Endpoint) {
        let _ = ask(endpoint, &Request::Quit);
        if let Self::Local { child: Some(child), .. } = self {
            let started = Instant::now();
            while child.try_wait().ok().flatten().is_none() {
                if started.elapsed() > Duration::from_secs(10) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// How one restart went.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Restart {
    /// From the save being seen to the state being restored.
    pub total_ms: f64,
    /// The build's share of it.
    pub build_ms: f64,
    /// State fields restored.
    pub restored: usize,
    /// State fields the component does not make editable.
    pub refused: usize,
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

fn build(project: &Project) -> Result<Duration> {
    let started = Instant::now();
    crate::diagnostics::run_cargo(&project.root, &["build".to_owned()])?;
    Ok(started.elapsed())
}

/// One rebuild and restart, keeping state.
fn restart(
    project: &Project,
    host: &mut Host,
    endpoint: &mut Endpoint,
    seen: Instant,
) -> Result<Restart> {
    let build_time = build(project)?;
    let states = snapshot(endpoint);
    host.stop(endpoint);
    *endpoint = host.start(&executable(project))?;
    let (restored, refused) = restore(endpoint, &states);
    let restart = Restart {
        total_ms: millis(seen.elapsed()),
        build_ms: millis(build_time),
        restored,
        refused,
    };
    let record = project.root.join("target").join("rustnative-dev");
    let _ = std::fs::create_dir_all(&record);
    let _ = std::fs::write(
        record.join("last-restart.json"),
        serde_json::to_vec_pretty(&restart).unwrap_or_default(),
    );
    Ok(restart)
}

/// Runs the loop for the project around `here`. With `once`, it makes one
/// restart after start-up — the application's main file saved again —
/// prints how it went as JSON, closes the application, and returns: what
/// the budget harness measures.
pub fn run(here: &Path, remote: Option<Remote>, once: bool) -> Result<()> {
    let project = Project::find(here)?;
    let resources = resource_env(&provision(&project)?);
    let mut host = match remote {
        Some(remote) => Host::Remote(remote),
        None => Host::Local { child: None, resources },
    };
    println!("dev: building {}", project.config.app.display_name);
    build(&project)?;
    let mut endpoint = host.start(&executable(&project))?;
    println!("dev: running; watching for changes (Ctrl+C to stop)");

    let outcome = watch(&project, &mut host, &mut endpoint, once);
    if outcome.is_err() || once {
        host.stop(&endpoint);
    }
    outcome
}

fn watch(project: &Project, host: &mut Host, endpoint: &mut Endpoint, once: bool) -> Result<()> {
    if once {
        let main = ["src/lib.rs", "src/main.rs"]
            .iter()
            .map(|file| project.root.join(file))
            .find(|path| path.is_file())
            .ok_or_else(|| Error::Usage("the project has no src/lib.rs or src/main.rs".into()))?;
        let text =
            std::fs::read(&main).map_err(|cause| Error::Io { what: "read main".into(), cause })?;
        std::fs::write(&main, text)
            .map_err(|cause| Error::Io { what: "save main".into(), cause })?;
        let outcome = restart(project, host, endpoint, Instant::now())?;
        println!("{}", serde_json::to_string(&outcome).unwrap_or_default());
        return Ok(());
    }

    let css_path = project.root.join("app.css");
    let mut css = std::fs::read_to_string(&css_path).ok();
    let mut files = scan(&project.root);
    loop {
        std::thread::sleep(POLL);
        let now = scan(&project.root);
        let paths = changed(&files, &now);
        if paths.is_empty() {
            continue;
        }
        let seen = Instant::now();
        // An editor's save can arrive in pieces.
        std::thread::sleep(Duration::from_millis(100));
        files = scan(&project.root);
        let new_css = std::fs::read_to_string(&css_path).ok();
        match classify(&paths, css.as_deref(), new_css.as_deref()) {
            Change::Theme => {
                let applied = ask(
                    endpoint,
                    &Request::SetStyleFile { css: new_css.clone().unwrap_or_default() },
                );
                match applied {
                    Ok(_) => println!(
                        "dev: app.css changed token values only — applied live in {:.0} ms, no rebuild",
                        millis(seen.elapsed())
                    ),
                    Err(error) => println!("dev: the theme could not be applied: {error}"),
                }
            }
            Change::Catalogues => {
                for path in &paths {
                    let locale = path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
                    let (Some(locale), Ok(ftl)) = (locale, std::fs::read_to_string(path)) else {
                        continue;
                    };
                    match ask(endpoint, &Request::SetCatalogue { locale: locale.clone(), ftl }) {
                        Ok(_) => println!(
                            "dev: locales/{locale}.ftl changed — applied live in {:.0} ms, no rebuild",
                            millis(seen.elapsed())
                        ),
                        Err(error) => println!("dev: locales/{locale}.ftl: {error}"),
                    }
                }
            }
            Change::Rebuild => {
                let names: Vec<String> = paths
                    .iter()
                    .map(|path| {
                        path.strip_prefix(&project.root).unwrap_or(path).display().to_string()
                    })
                    .collect();
                println!("dev: {} changed — rebuilding", names.join(", "));
                match restart(project, host, endpoint, seen) {
                    Ok(done) => println!(
                        "dev: restarted in {:.0} ms (build {:.0} ms); state: {} restored, {} not editable",
                        done.total_ms, done.build_ms, done.restored, done.refused
                    ),
                    Err(error) => {
                        println!("dev: {error} — the running application is unchanged");
                    }
                }
            }
        }
        css = new_css;
    }
}

/// `rustnative test --watch`: runs `command` now and again after every save
/// under `src/` or `tests/`. The project is one crate, so a save affects
/// the whole of it.
///
/// # Errors
///
/// Cargo cannot be run at all.
pub fn watch_tests(project: &Project, command: &[String]) -> Result<()> {
    let files = |root: &Path| {
        let mut files = scan(root);
        walk(&root.join("tests"), &mut files);
        files
    };
    let mut seen = files(&project.root);
    loop {
        match crate::diagnostics::run_cargo(&project.root, command) {
            Ok(()) => println!("test: passed — watching for changes (Ctrl+C to stop)"),
            Err(error) => println!("test: {error} — watching for changes (Ctrl+C to stop)"),
        }
        loop {
            std::thread::sleep(POLL);
            let now = files(&project.root);
            if !changed(&seen, &now).is_empty() {
                seen = now;
                break;
            }
        }
    }
}

// ----------------------------------------------------------------------
// The agent
// ----------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Deploy {
    token: String,
    name: String,
    size: u64,
    #[serde(default)]
    args: Vec<String>,
}

/// What the agent answers a deployment with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployed {
    /// The started process.
    pub pid: u32,
    /// Its inspection endpoint, reachable from the developer's machine.
    pub endpoint: Option<Endpoint>,
    /// Its exit code, if it exited before publishing one.
    pub exited: Option<i32>,
    /// Why the deployment failed.
    #[serde(default)]
    pub error: Option<String>,
}

/// Sends `executable` to the agent at `remote` and has it started with
/// `args`.
///
/// # Errors
///
/// The agent cannot be reached, refuses the token, or cannot start it.
pub fn deploy(remote: &Remote, executable: &Path, args: &[String]) -> Result<Deployed> {
    let bytes = std::fs::read(executable)
        .map_err(|cause| Error::Io { what: format!("read {}", executable.display()), cause })?;
    let io = |cause| Error::Io { what: format!("deploy to {}", remote.addr), cause };
    let mut stream = TcpStream::connect(remote.addr).map_err(io)?;
    let header = Deploy {
        token: remote.token.clone(),
        name: executable
            .file_name()
            .map_or_else(|| "app".to_owned(), |name| name.to_string_lossy().into_owned()),
        size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        args: args.to_vec(),
    };
    writeln!(stream, "{}", serde_json::to_string(&header).unwrap_or_default()).map_err(io)?;
    // The agent answers the header first — ready, or why not — so a refused
    // deployment is not sent at all.
    let mut reader = BufReader::new(stream.try_clone().map_err(io)?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(io)?;
    if !line.contains("\"ready\"") {
        let refused: Deployed = serde_json::from_str(&line).map_err(|error| {
            Error::Usage(format!("the agent's answer is not readable: {error}"))
        })?;
        return Err(Error::Usage(format!("the agent says: {}", refused.error.unwrap_or_default())));
    }
    stream.write_all(&bytes).map_err(io)?;
    line.clear();
    reader.read_line(&mut line).map_err(io)?;
    let deployed: Deployed = serde_json::from_str(&line)
        .map_err(|error| Error::Usage(format!("the agent's answer is not readable: {error}")))?;
    match deployed.error {
        Some(error) => Err(Error::Usage(format!("the agent says: {error}"))),
        None => Ok(deployed),
    }
}

/// Runs the agent on `listen`: receives builds, starts them, and hands back
/// their inspection endpoints. It runs what a token holder sends it, so it
/// binds loopback unless told otherwise, and prints the token once.
///
/// # Errors
///
/// The address cannot be bound.
pub fn agent(listen: SocketAddr, max_deployments: Option<usize>) -> Result<()> {
    let listener = TcpListener::bind(listen)
        .map_err(|cause| Error::Io { what: format!("listen on {listen}"), cause })?;
    let addr = listener
        .local_addr()
        .map_err(|cause| Error::Io { what: "read the agent's address".into(), cause })?;
    let token = rustnative_core::inspect::new_token()
        .map_err(|cause| Error::Io { what: "generate the agent's token".into(), cause })?;
    println!("rustnative dev-agent listening on {addr} token {token}");
    let _ = std::io::stdout().flush();
    let folder = std::env::temp_dir().join("rustnative-dev-agent");
    let mut running: Option<Child> = None;
    let mut served = 0;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let reply = serve_deployment(stream.try_clone().ok(), &token, &folder, addr, &mut running);
        if let Ok(mut stream) = stream.try_clone() {
            let _ = writeln!(stream, "{}", serde_json::to_string(&reply).unwrap_or_default());
        }
        served += 1;
        if max_deployments.is_some_and(|max| served >= max) {
            break;
        }
    }
    if let Some(mut child) = running {
        let _ = child.kill();
    }
    Ok(())
}

fn serve_deployment(
    stream: Option<TcpStream>,
    token: &str,
    folder: &Path,
    listen: SocketAddr,
    running: &mut Option<Child>,
) -> Deployed {
    let failed =
        |error: String| Deployed { pid: 0, endpoint: None, exited: None, error: Some(error) };
    let Some(stream) = stream else { return failed("no connection".into()) };
    let local_ip = stream.local_addr().map(|addr| addr.ip()).ok();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return failed("no header".into());
    }
    let Ok(header) = serde_json::from_str::<Deploy>(&line) else {
        return failed("the header is not a deployment".into());
    };
    let same = header.token.len() == token.len()
        && header.token.bytes().zip(token.bytes()).fold(0, |acc, (a, b)| acc | (a ^ b)) == 0;
    if !same {
        return failed("wrong token".into());
    }
    let Ok(size) = usize::try_from(header.size) else { return failed("too large".into()) };
    if writeln!(reader.get_mut(), "{{\"ready\":true}}").is_err() {
        return failed("the connection closed".into());
    }
    let mut bytes = vec![0; size];
    if reader.read_exact(&mut bytes).is_err() {
        return failed("the executable was cut short".into());
    }
    // Only a file name: the agent writes nowhere but its own folder.
    let Some(name) = Path::new(&header.name).file_name() else {
        return failed("no executable name".into());
    };
    if let Some(mut previous) = running.take() {
        let _ = previous.kill();
        let _ = previous.wait();
    }
    let path = folder.join(name);
    if let Err(error) = std::fs::create_dir_all(folder).and_then(|()| std::fs::write(&path, &bytes))
    {
        return failed(format!("cannot write {}: {error}", path.display()));
    }
    // Runnable where the file system has an execute bit.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if let Err(error) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        {
            return failed(format!("cannot make {} executable: {error}", path.display()));
        }
    }
    let bind = SocketAddr::new(listen.ip(), 0);
    let child = Command::new(&path)
        .args(&header.args)
        .env(rustnative_core::inspect::INSPECT_VARIABLE, bind.to_string())
        .env(rustnative_core::dev::DEV_VARIABLE, "1")
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return failed(format!("cannot start {}: {error}", path.display())),
    };
    let pid = child.id();
    let mut exited = None;
    let endpoint = wait_for_endpoint(pid, || {
        child.try_wait().ok().flatten().map(|status| {
            exited = status.code();
            status.to_string()
        })
    })
    .ok()
    .map(|mut endpoint| {
        // Bound to every interface: named by the one the developer reached.
        if endpoint.addr.ip().is_unspecified() {
            if let Some(ip) = local_ip {
                endpoint.addr.set_ip(ip);
            }
        }
        endpoint
    });
    *running = Some(child);
    Deployed { pid, endpoint, exited, error: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSS: &str = "@theme {\n  --color-accent: oklch(0.55 0.19 255);\n}\n\n@utility headline {\n  @apply text-lg font-semibold text-accent;\n}\n";

    #[test]
    fn a_token_value_is_live_and_a_utility_is_a_rebuild() {
        let css = [PathBuf::from("app.css")];
        let token = CSS.replace("0.55", "0.65");
        assert_eq!(classify(&css, Some(CSS), Some(&token)), Change::Theme);
        let utility = format!("{CSS}\n@utility card {{\n  @apply p-4;\n}}\n");
        assert_eq!(classify(&css, Some(CSS), Some(&utility)), Change::Rebuild);
        let renamed = CSS.replace("--color-accent", "--color-brand");
        assert_eq!(classify(&css, Some(CSS), Some(&renamed)), Change::Rebuild, "a new token name");
        let inline = CSS.replace("@theme {", "@theme inline {");
        let inline_token = inline.replace("0.55", "0.65");
        assert_eq!(
            classify(&css, Some(&inline), Some(&inline_token)),
            Change::Rebuild,
            "an inline theme is folded at build time"
        );
        let locales = [PathBuf::from("locales/pl.ftl"), PathBuf::from("locales/ar.ftl")];
        assert_eq!(classify(&locales, None, None), Change::Catalogues, "translations are live");
        let mixed = [PathBuf::from("locales/pl.ftl"), PathBuf::from("src/lib.rs")];
        assert_eq!(classify(&mixed, None, None), Change::Rebuild);
        let both = [PathBuf::from("app.css"), PathBuf::from("src/lib.rs")];
        assert_eq!(classify(&both, Some(CSS), Some(&token)), Change::Rebuild);
        assert_eq!(classify(&css, Some(CSS), Some("@theme {")), Change::Rebuild, "unparsable");
    }

    #[test]
    fn changes_are_what_is_new_modified_or_gone() {
        let now = SystemTime::now();
        let later = now + Duration::from_secs(1);
        let before = BTreeMap::from([(PathBuf::from("a"), now), (PathBuf::from("b"), now)]);
        let after = BTreeMap::from([(PathBuf::from("a"), later), (PathBuf::from("c"), now)]);
        let mut found = changed(&before, &after);
        found.sort();
        assert_eq!(found, [PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")]);
    }
}
