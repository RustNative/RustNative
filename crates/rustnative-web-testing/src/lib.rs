//! Browser tests for the Web backend (`PLAN.md` §9: "browser tests for the
//! Web backend"; Web milestone J: browser test execution and accessibility
//! testing).
//!
//! [`Browser::launch`] starts a headless Chromium-family browser — the one
//! `RUSTNATIVE_BROWSER` names, else Microsoft Edge or Google Chrome where
//! they install by default — and drives it over the `DevTools` protocol:
//! navigation, script evaluation, real input events dispatched through the
//! browser's own input pipeline, network and CPU emulation, and the
//! browser's accessibility tree, which is what assistive technology reads.
//!
//! A machine with no such browser gets `Ok(None)`, and a test prints that
//! it was skipped rather than failing — `BUILD_STATUS.md` records where the
//! browser suite last ran.
//!
//! [`StaticServer`] serves fixed responses on a loopback port, for pages a
//! test builds itself.

#![deny(missing_docs)]

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

/// Why a browser step failed.
#[derive(Debug)]
pub enum Error {
    /// Starting or talking to the browser failed.
    Io(std::io::Error),
    /// The protocol connection failed.
    Socket(String),
    /// The browser answered a command with an error.
    Protocol(String),
    /// Script threw.
    Script(String),
    /// Something did not happen in time.
    Timeout(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "browser I/O: {error}"),
            Self::Socket(error) => write!(f, "DevTools connection: {error}"),
            Self::Protocol(error) => write!(f, "DevTools answered: {error}"),
            Self::Script(error) => write!(f, "script threw: {error}"),
            Self::Timeout(what) => write!(f, "timed out waiting for {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// What a browser step returns.
pub type Result<T> = std::result::Result<T, Error>;

/// Where a Chromium-family browser is installed, if one is.
#[must_use]
pub fn find_browser() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RUSTNATIVE_BROWSER") {
        return Some(PathBuf::from(path));
    }
    let candidates: &[&str] = &[
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/microsoft-edge",
    ];
    candidates.iter().map(PathBuf::from).find(|path| path.is_file())
}

/// Where Node.js is, if it is installed: the generated-JavaScript unit
/// tests run their programs in it.
#[must_use]
pub fn find_node() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RUSTNATIVE_NODE") {
        return Some(PathBuf::from(path));
    }
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

struct Connection {
    socket: WebSocket<TcpStream>,
    next: u64,
    events: VecDeque<Value>,
}

impl Connection {
    fn read(&mut self, deadline: Instant) -> Result<Option<Value>> {
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str(&text)
                        .map(Some)
                        .map_err(|error| Error::Socket(error.to_string()));
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
                Err(error) => return Err(Error::Socket(error.to_string())),
            }
        }
    }

    fn call(&mut self, session: Option<&str>, method: &str, params: &Value) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        let mut command = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            command["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::Text(command.to_string()))
            .map_err(|error| Error::Socket(error.to_string()))?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let Some(message) = self.read(deadline)? else {
                return Err(Error::Timeout(format!("an answer to {method}")));
            };
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(Error::Protocol(format!("{method}: {error}")));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            self.events.push_back(message);
        }
    }

    fn wait_event(&mut self, session: &str, method: &str, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(index) = self.events.iter().position(|event| {
                event.get("method").and_then(Value::as_str) == Some(method)
                    && event.get("sessionId").and_then(Value::as_str) == Some(session)
            }) {
                return Ok(self.events.remove(index).unwrap_or(Value::Null));
            }
            match self.read(deadline)? {
                Some(message) => self.events.push_back(message),
                None => return Err(Error::Timeout(method.to_owned())),
            }
        }
    }

    fn drain(&mut self) {
        let deadline = Instant::now();
        while let Ok(Some(message)) = self.read(deadline) {
            self.events.push_back(message);
        }
    }
}

/// A running headless browser.
pub struct Browser {
    child: Child,
    profile: PathBuf,
    connection: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Browser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser").field("profile", &self.profile).finish_non_exhaustive()
    }
}

static PROFILES: AtomicU64 = AtomicU64::new(0);

impl Browser {
    /// Starts a headless browser, or answers `None` when this machine has
    /// none ([`find_browser`]).
    ///
    /// # Errors
    ///
    /// The browser was found but did not start or did not open its
    /// `DevTools` endpoint.
    pub fn launch() -> Result<Option<Self>> {
        let Some(executable) = find_browser() else { return Ok(None) };
        Self::launch_with(&executable, &[]).map(Some)
    }

    /// Starts `executable` headless with `extra` arguments.
    ///
    /// # Errors
    ///
    /// It did not start or did not open its `DevTools` endpoint.
    pub fn launch_with(executable: &Path, extra: &[&str]) -> Result<Self> {
        let profile = std::env::temp_dir().join(format!(
            "rustnative-browser-{}-{}",
            std::process::id(),
            PROFILES.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&profile);
        std::fs::create_dir_all(&profile)?;
        let mut command = Command::new(executable);
        command
            .arg("--headless=new")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-background-networking",
                "--disable-sync",
                "--disable-component-update",
                "--mute-audio",
                "--window-size=1024,768",
            ])
            .args(extra)
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn()?;
        let port_file = profile.join("DevToolsActivePort");
        let deadline = Instant::now() + Duration::from_secs(30);
        let (port, path) = loop {
            if let Ok(text) = std::fs::read_to_string(&port_file) {
                let mut lines = text.lines();
                if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                    if let Ok(port) = port.trim().parse::<u16>() {
                        break (port, path.trim().to_owned());
                    }
                }
            }
            if Instant::now() > deadline {
                return Err(Error::Timeout("the browser's DevTools endpoint".into()));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let stream = TcpStream::connect(("127.0.0.1", port))?;
        let url = format!("ws://127.0.0.1:{port}{path}");
        let (socket, _) = tungstenite::client(url.as_str(), stream)
            .map_err(|error| Error::Socket(error.to_string()))?;
        socket.get_ref().set_read_timeout(Some(Duration::from_millis(50)))?;
        Ok(Self {
            child,
            profile,
            connection: Arc::new(Mutex::new(Connection {
                socket,
                next: 0,
                events: VecDeque::new(),
            })),
        })
    }

    /// Opens a new page (a tab) at `about:blank`.
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn page(&self) -> Result<Page> {
        let mut connection = self.connection.lock().unwrap_or_else(PoisonError::into_inner);
        let target =
            connection.call(None, "Target.createTarget", &json!({ "url": "about:blank" }))?;
        let target_id = target["targetId"].as_str().unwrap_or_default().to_owned();
        let attached = connection.call(
            None,
            "Target.attachToTarget",
            &json!({ "targetId": target_id, "flatten": true }),
        )?;
        let session = attached["sessionId"].as_str().unwrap_or_default().to_owned();
        for domain in ["Page.enable", "Runtime.enable", "Network.enable"] {
            connection.call(Some(&session), domain, &json!({}))?;
        }
        drop(connection);
        Ok(Page {
            connection: Arc::clone(&self.connection),
            session,
            target: target_id,
            workers: Mutex::new(HashMap::new()),
        })
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Ok(mut connection) = self.connection.lock() {
            let _ = connection.call(None, "Browser.close", &json!({}));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

/// One node of the browser's accessibility tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxNode {
    /// Its role, as the browser computes it (`button`, `heading`, …).
    pub role: String,
    /// Its accessible name.
    pub name: String,
    /// Whether the browser leaves it out of what assistive technology sees.
    pub ignored: bool,
    /// Its properties (`checked`, `level`, `disabled`, …) as text.
    pub properties: Vec<(String, String)>,
}

/// One page of a [`Browser`].
pub struct Page {
    connection: Arc<Mutex<Connection>>,
    session: String,
    target: String,
    /// Sessions on service workers, whose network this page emulates.
    workers: Mutex<HashMap<String, String>>,
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Page").field("target", &self.target).finish_non_exhaustive()
    }
}

fn key_code(key: &str) -> (i64, &str) {
    match key {
        "Tab" => (9, "Tab"),
        "Enter" => (13, "Enter"),
        "Escape" => (27, "Escape"),
        "Backspace" => (8, "Backspace"),
        "Space" | " " => (32, "Space"),
        "ArrowLeft" => (37, "ArrowLeft"),
        "ArrowUp" => (38, "ArrowUp"),
        "ArrowRight" => (39, "ArrowRight"),
        "ArrowDown" => (40, "ArrowDown"),
        "Delete" => (46, "Delete"),
        "Home" => (36, "Home"),
        "End" => (35, "End"),
        _ => (0, ""),
    }
}

impl Page {
    /// Runs a `DevTools` command on this page.
    ///
    /// # Errors
    ///
    /// The browser refused it.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "call sites read as `call(method, json!({ .. }))`, the protocol's own shape"
    )]
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.connection.lock().unwrap_or_else(PoisonError::into_inner).call(
            Some(&self.session),
            method,
            &params,
        )
    }

    /// Waits for the next `method` event on this page.
    ///
    /// # Errors
    ///
    /// It did not arrive within `timeout`.
    pub fn wait_event(&self, method: &str, timeout: Duration) -> Result<Value> {
        self.connection.lock().unwrap_or_else(PoisonError::into_inner).wait_event(
            &self.session,
            method,
            timeout,
        )
    }

    /// Every event the page raised so far that nobody waited for, taken.
    #[must_use]
    pub fn take_events(&self, method: &str) -> Vec<Value> {
        let mut connection = self.connection.lock().unwrap_or_else(PoisonError::into_inner);
        connection.drain();
        let session = self.session.clone();
        let mut taken = Vec::new();
        connection.events.retain(|event| {
            let matches = event.get("method").and_then(Value::as_str) == Some(method)
                && event.get("sessionId").and_then(Value::as_str) == Some(session.as_str());
            if matches {
                taken.push(event.clone());
            }
            !matches
        });
        taken
    }

    /// Navigates to `url` and waits for its `load` event.
    ///
    /// # Errors
    ///
    /// The navigation failed or did not load within 30 seconds.
    pub fn goto(&self, url: &str) -> Result<()> {
        let _ = self.take_events("Page.loadEventFired");
        let result = self.call("Page.navigate", json!({ "url": url }))?;
        if let Some(error) = result.get("errorText").and_then(Value::as_str) {
            return Err(Error::Protocol(format!("navigating to {url}: {error}")));
        }
        self.wait_event("Page.loadEventFired", Duration::from_secs(30))?;
        Ok(())
    }

    /// Evaluates `expression` (awaiting it if it is a promise) and returns
    /// its value as JSON.
    ///
    /// # Errors
    ///
    /// The script threw.
    pub fn eval(&self, expression: &str) -> Result<Value> {
        let result = self.call(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
        )?;
        if let Some(details) = result.get("exceptionDetails") {
            let text = details
                .pointer("/exception/description")
                .and_then(Value::as_str)
                .or_else(|| details.get("text").and_then(Value::as_str))
                .unwrap_or("an exception");
            return Err(Error::Script(text.to_owned()));
        }
        Ok(result.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    /// Waits until `expression` is truthy.
    ///
    /// # Errors
    ///
    /// It was not within `timeout`, or it threw.
    pub fn wait_until(&self, expression: &str, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let value = self.eval(expression)?;
            let truthy = match &value {
                Value::Null => false,
                Value::Bool(value) => *value,
                Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
                Value::String(text) => !text.is_empty(),
                _ => true,
            };
            if truthy {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(Error::Timeout(expression.to_owned()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The outer HTML of the first element matching `selector`.
    ///
    /// # Errors
    ///
    /// No element matches.
    pub fn html(&self, selector: &str) -> Result<String> {
        let value = self.eval(&format!(
            "(() => {{ const e = document.querySelector({}); return e ? e.outerHTML : null; }})()",
            json!(selector)
        ))?;
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::Script(format!("no element matches {selector}")))
    }

    /// The center of the first element matching `selector`, scrolled into
    /// view, in viewport coordinates.
    ///
    /// # Errors
    ///
    /// No element matches.
    pub fn center(&self, selector: &str) -> Result<(f64, f64)> {
        let value = self.eval(&format!(
            "(() => {{ const e = document.querySelector({}); if (!e) return null; e.scrollIntoView({{block: 'center'}}); \
             const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; }})()",
            json!(selector)
        ))?;
        match (value.get(0).and_then(Value::as_f64), value.get(1).and_then(Value::as_f64)) {
            (Some(x), Some(y)) => Ok((x, y)),
            _ => Err(Error::Script(format!("no element matches {selector}"))),
        }
    }

    /// Clicks the center of the first element matching `selector` with the
    /// primary mouse button, through the browser's input pipeline.
    ///
    /// # Errors
    ///
    /// No element matches, or the browser refused the input.
    pub fn click(&self, selector: &str) -> Result<()> {
        let (x, y) = self.center(selector)?;
        self.mouse("mouseMoved", x, y, 0)?;
        self.mouse("mousePressed", x, y, 1)?;
        self.mouse("mouseReleased", x, y, 1)?;
        Ok(())
    }

    /// Dispatches one mouse event at `(x, y)`.
    ///
    /// # Errors
    ///
    /// The browser refused it.
    pub fn mouse(&self, kind: &str, x: f64, y: f64, clicks: u8) -> Result<()> {
        let button = if kind == "mouseMoved" && clicks == 0 { "none" } else { "left" };
        self.call(
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": x, "y": y, "button": button, "buttons": u8::from(kind == "mousePressed"), "clickCount": clicks }),
        )?;
        Ok(())
    }

    /// Focuses the first element matching `selector`.
    ///
    /// # Errors
    ///
    /// No element matches.
    pub fn focus(&self, selector: &str) -> Result<()> {
        let found = self.eval(&format!(
            "(() => {{ const e = document.querySelector({}); if (!e) return false; e.focus(); return true; }})()",
            json!(selector)
        ))?;
        if found == Value::Bool(true) {
            Ok(())
        } else {
            Err(Error::Script(format!("no element matches {selector}")))
        }
    }

    /// Types `text` into the focused element as the browser's own text
    /// input would.
    ///
    /// # Errors
    ///
    /// The browser refused it.
    pub fn type_text(&self, text: &str) -> Result<()> {
        for character in text.chars() {
            self.call("Input.insertText", json!({ "text": character.to_string() }))?;
        }
        Ok(())
    }

    /// Presses and releases `key` (`"Tab"`, `"Enter"`, `"ArrowDown"`, or a
    /// character).
    ///
    /// # Errors
    ///
    /// The browser refused it.
    pub fn press(&self, key: &str) -> Result<()> {
        self.press_with(key, 0)
    }

    /// Presses `key` with modifiers (1 Alt, 2 Control, 4 Meta, 8 Shift).
    ///
    /// # Errors
    ///
    /// The browser refused it.
    pub fn press_with(&self, key: &str, modifiers: u8) -> Result<()> {
        let (code, name) = key_code(key);
        let (key_name, text) = if code == 0 {
            (key.to_owned(), key.to_owned())
        } else {
            (name.to_owned(), String::new())
        };
        let text = if key == "Enter" { "\r".to_owned() } else { text };
        let mut down = json!({ "type": "keyDown", "key": key_name, "code": name, "windowsVirtualKeyCode": code, "modifiers": modifiers });
        if !text.is_empty() {
            down["text"] = json!(text);
        }
        self.call("Input.dispatchKeyEvent", down)?;
        self.call(
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": key_name, "code": name, "windowsVirtualKeyCode": code, "modifiers": modifiers }),
        )?;
        Ok(())
    }

    /// The browser's accessibility tree for the page, in document order.
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn accessibility_tree(&self) -> Result<Vec<AxNode>> {
        self.call("Accessibility.enable", json!({}))?;
        let tree = self.call("Accessibility.getFullAXTree", json!({}))?;
        let text = |value: Option<&Value>| {
            value.and_then(|value| value.get("value")).map_or_else(String::new, |value| match value
            {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
        };
        Ok(tree["nodes"]
            .as_array()
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|node| AxNode {
                        role: text(node.get("role")),
                        name: text(node.get("name")),
                        ignored: node.get("ignored").and_then(Value::as_bool).unwrap_or(false),
                        properties: node
                            .get("properties")
                            .and_then(Value::as_array)
                            .map(|properties| {
                                properties
                                    .iter()
                                    .map(|property| {
                                        (
                                            property["name"]
                                                .as_str()
                                                .unwrap_or_default()
                                                .to_owned(),
                                            text(property.get("value")),
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The first unignored accessibility node with `role` and `name`.
    ///
    /// # Errors
    ///
    /// None matches; the error lists what the tree has.
    pub fn find_accessible(&self, role: &str, name: &str) -> Result<AxNode> {
        let tree = self.accessibility_tree()?;
        tree.iter()
            .find(|node| !node.ignored && node.role == role && node.name == name)
            .cloned()
            .ok_or_else(|| {
                let seen: Vec<String> = tree
                    .iter()
                    .filter(|node| !node.ignored && !node.name.is_empty())
                    .map(|node| format!("{} {:?}", node.role, node.name))
                    .collect();
                Error::Script(format!(
                    "no {role} named {name:?}; the tree has: {}",
                    seen.join(", ")
                ))
            })
    }

    /// Messages the page logged to its console, and exceptions it threw,
    /// since the last call.
    #[must_use]
    pub fn console(&self) -> Vec<String> {
        let mut messages: Vec<String> = self
            .take_events("Runtime.consoleAPICalled")
            .iter()
            .map(|event| {
                let kind = event.pointer("/params/type").and_then(Value::as_str).unwrap_or("log");
                let args: Vec<String> = event
                    .pointer("/params/args")
                    .and_then(Value::as_array)
                    .map(|args| {
                        args.iter()
                            .map(|arg| {
                                arg.get("value").map_or_else(
                                    || {
                                        arg.get("description")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default()
                                            .to_owned()
                                    },
                                    |value| {
                                        value
                                            .as_str()
                                            .map_or_else(|| value.to_string(), str::to_owned)
                                    },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                format!("{kind}: {}", args.join(" "))
            })
            .collect();
        messages.extend(self.take_events("Runtime.exceptionThrown").iter().map(|event| {
            let text = event
                .pointer("/params/exceptionDetails/exception/description")
                .or_else(|| event.pointer("/params/exceptionDetails/text"))
                .and_then(Value::as_str)
                .unwrap_or("an exception");
            format!("exception: {text}")
        }));
        messages
    }

    /// Emulates the network as offline (or not).
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn set_offline(&self, offline: bool) -> Result<()> {
        self.call(
            "Network.emulateNetworkConditions",
            json!({ "offline": offline, "latency": 0, "downloadThroughput": -1, "uploadThroughput": -1 }),
        )?;
        Ok(())
    }

    /// Emulates the network as offline (or not) for this page and for every
    /// service worker the browser runs — whose requests a page's own
    /// emulation does not reach.
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn set_offline_everywhere(&self, offline: bool) -> Result<()> {
        self.set_offline(offline)?;
        let mut connection = self.connection.lock().unwrap_or_else(PoisonError::into_inner);
        let targets = connection.call(None, "Target.getTargets", &json!({}))?;
        let workers: Vec<String> = targets["targetInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|target| target["type"] == "service_worker")
            .filter_map(|target| target["targetId"].as_str().map(str::to_owned))
            .collect();
        let mut sessions = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        for target in workers {
            // One session per worker: emulation set through a session stays
            // until that session changes it.
            let session = if let Some(session) = sessions.get(&target) {
                session.clone()
            } else {
                let attached = connection.call(
                    None,
                    "Target.attachToTarget",
                    &json!({ "targetId": target, "flatten": true }),
                )?;
                let Some(session) = attached["sessionId"].as_str().map(str::to_owned) else {
                    continue;
                };
                connection.call(Some(&session), "Network.enable", &json!({}))?;
                sessions.insert(target, session.clone());
                session
            };
            connection.call(
                Some(&session),
                "Network.emulateNetworkConditions",
                &json!({ "offline": offline, "latency": 0, "downloadThroughput": -1, "uploadThroughput": -1 }),
            )?;
        }
        Ok(())
    }

    /// Slows the CPU by `rate` (4 is the conventional low-end profile) and
    /// the network to `latency_ms` and `kbps` (`None`: unthrottled).
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn throttle(&self, rate: f64, network: Option<(u32, u32)>) -> Result<()> {
        self.call("Emulation.setCPUThrottlingRate", json!({ "rate": rate }))?;
        if let Some((latency, kbps)) = network {
            let bytes = f64::from(kbps) * 1024.0 / 8.0;
            self.call(
                "Network.emulateNetworkConditions",
                json!({ "offline": false, "latency": latency, "downloadThroughput": bytes, "uploadThroughput": bytes }),
            )?;
        }
        Ok(())
    }

    /// Turns script execution off (or back on) for this page: what a
    /// progressive-enhancement test browses with.
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn set_script_enabled(&self, enabled: bool) -> Result<()> {
        self.call("Emulation.setScriptExecutionDisabled", json!({ "value": !enabled }))?;
        Ok(())
    }

    /// Emulates `prefers-color-scheme` (`"dark"`, `"light"`) and a viewport
    /// width.
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn emulate(&self, scheme: Option<&str>, width: Option<u32>) -> Result<()> {
        if let Some(scheme) = scheme {
            self.call(
                "Emulation.setEmulatedMedia",
                json!({ "features": [{ "name": "prefers-color-scheme", "value": scheme }] }),
            )?;
        }
        if let Some(width) = width {
            self.call(
                "Emulation.setDeviceMetricsOverride",
                json!({ "width": width, "height": 800, "deviceScaleFactor": 1, "mobile": false }),
            )?;
        }
        Ok(())
    }

    /// Grants the page's origin `permissions` (`"clipboardReadWrite"`, …).
    ///
    /// # Errors
    ///
    /// The browser refused.
    pub fn grant(&self, origin: &str, permissions: &[&str]) -> Result<()> {
        self.connection.lock().unwrap_or_else(PoisonError::into_inner).call(
            None,
            "Browser.grantPermissions",
            &json!({ "origin": origin, "permissions": permissions }),
        )?;
        Ok(())
    }
}

/// A fixed response a [`StaticServer`] gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The status.
    pub status: u16,
    /// Its headers.
    pub headers: Vec<(String, String)>,
    /// Its body.
    pub body: Vec<u8>,
}

impl Response {
    /// `200` with `content_type` and `body`.
    #[must_use]
    pub fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".into(), content_type.into())],
            body: body.into(),
        }
    }

    /// With another header.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

type Routes = Arc<Mutex<HashMap<String, Response>>>;

/// Serves fixed responses on a loopback port until dropped.
#[derive(Debug)]
pub struct StaticServer {
    port: u16,
    routes: Routes,
}

impl StaticServer {
    /// Starts serving (nothing yet: every path is `404`).
    ///
    /// # Errors
    ///
    /// No loopback port could be bound.
    pub fn start() -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let routes: Routes = Arc::default();
        let served = Arc::clone(&routes);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let routes = Arc::clone(&served);
                std::thread::spawn(move || serve(stream, &routes));
            }
        });
        Ok(Self { port, routes })
    }

    /// Answers `path` (with or without a query) with `response`.
    pub fn set(&self, path: &str, response: Response) {
        self.routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_owned(), response);
    }

    /// `http://127.0.0.1:<port>`.
    #[must_use]
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The URL of `path`.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin())
    }
}

fn serve(stream: TcpStream, routes: &Routes) {
    let Ok(clone) = stream.try_clone() else { return };
    let mut reader = BufReader::new(clone);
    let mut writer = stream;
    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_owned();
        let target = parts.next().unwrap_or("/").to_owned();
        let mut length = 0_usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().unwrap_or(0);
                }
            }
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let path = target.split('?').next().unwrap_or("/");
        let response = {
            let routes = routes.lock().unwrap_or_else(PoisonError::into_inner);
            routes.get(&target).or_else(|| routes.get(path)).cloned()
        }
        .unwrap_or(Response {
            status: 404,
            headers: vec![("content-type".into(), "text/plain".into())],
            body: b"not found".to_vec(),
        });
        let mut head = format!(
            "HTTP/1.1 {} X\r\ncontent-length: {}\r\n",
            response.status,
            response.body.len()
        );
        for (name, value) in &response.headers {
            let _ = write!(head, "{name}: {value}\r\n");
        }
        head.push_str("\r\n");
        if writer.write_all(head.as_bytes()).is_err() {
            return;
        }
        if method != "HEAD" && writer.write_all(&response.body).is_err() {
            return;
        }
    }
}

/// Launches a browser for a test, or prints why the test is skipped and
/// returns `None`.
///
/// # Panics
///
/// A browser was found but would not start: a broken installation is a
/// failure, not a skip.
#[must_use]
#[allow(clippy::panic, reason = "a test helper: a browser that will not start fails the test")]
pub fn browser_or_skip(test: &str) -> Option<Browser> {
    match Browser::launch() {
        Ok(Some(browser)) => Some(browser),
        Ok(None) => {
            // `rustnative test --browser`: the browser tests must run.
            assert!(
                std::env::var_os("RUSTNATIVE_BROWSER_REQUIRED").is_none(),
                "{test}: no Chromium-family browser, and the browser tests are required (set RUSTNATIVE_BROWSER)"
            );
            eprintln!("skipped {test}: no Chromium-family browser (set RUSTNATIVE_BROWSER)");
            None
        }
        Err(error) => panic!("{test}: the browser did not start: {error}"),
    }
}
