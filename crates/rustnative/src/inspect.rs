//! `rustnative inspect`: the inspector client (`PLAN.md` Milestone 44).
//!
//! It finds a running application's inspection endpoint — the one named on
//! the command line, or the one the application published when started
//! with `RUSTNATIVE_INSPECT=1` — sends one request, and prints the answer
//! as text (or, with `--json`, as the protocol's JSON). Every backend
//! answers the same protocol, so the client does not know which it talks
//! to.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use rustnative_core::inspect::{
    Endpoint, OverlayMode, PROTOCOL_VERSION, Recording, Reply, Request, endpoint_directory,
    send_request,
};
use serde_json::Value;

use crate::error::{Error, Result};

/// Where to find the application.
#[derive(Debug, clap::Args)]
pub struct Target {
    /// The endpoint's address (with `--token`); otherwise the most recently
    /// started inspectable application on this machine.
    #[arg(long, requires = "token")]
    addr: Option<SocketAddr>,
    /// The endpoint's token.
    #[arg(long, requires = "addr")]
    token: Option<String>,
    /// The process to inspect, among those that published an endpoint.
    #[arg(long, conflicts_with = "addr")]
    pid: Option<u32>,
    /// Print the protocol's JSON rather than text.
    #[arg(long)]
    json: bool,
    /// The application on the attached Android device (started with
    /// `rustnative run android --inspect`): its port is forwarded with
    /// `adb forward`, and its endpoint read with `run-as`.
    #[arg(long, conflicts_with_all = ["addr", "pid"])]
    android: bool,
}

/// What to ask.
#[derive(Debug, Subcommand)]
pub enum Question {
    /// The protocol version, the backend, and the windows.
    Hello,
    /// The declarative tree, with each node's component and classes.
    Tree {
        /// The window.
        #[arg(long)]
        window: Option<u64>,
    },
    /// Every component: path, type, state, tasks.
    Components {
        /// The window.
        #[arg(long)]
        window: Option<u64>,
    },
    /// The host objects realizing the tree.
    Realized {
        /// The window.
        #[arg(long)]
        window: Option<u64>,
    },
    /// One component's state.
    State {
        /// The component's key path.
        path: String,
    },
    /// Sets a field of a component's state (as JSON: `5`, `"text"`).
    Set {
        /// The component's key path.
        path: String,
        /// The field.
        field: String,
        /// The value, as JSON.
        value: String,
    },
    /// Why a node has its geometry.
    Explain {
        /// The node: a key, `component path::key`, or an id.
        node: String,
    },
    /// Where each of a node's style properties came from.
    Style {
        /// The node: a key, `component path::key`, or an id.
        node: String,
    },
    /// Events and task deliveries, with every component's render-or-skip
    /// reason.
    Trace {
        /// The first sequence number to show.
        #[arg(long, default_value_t = 0)]
        since: u64,
    },
    /// Tasks by owning component.
    Tasks,
    /// Host objects created and destroyed.
    Lifetimes,
    /// The live inspectable stores and their values.
    Stores,
    /// What the host advertises and what it refused.
    Caps,
    /// Active mapper customizations.
    Mappers,
    /// Inspectable state after each change, oldest first.
    History {
        /// Only this component, by key path.
        #[arg(long)]
        component: Option<String>,
    },
    /// Shows or hides the in-application overlay.
    Overlay {
        /// What it draws.
        mode: OverlayChoice,
    },
    /// Starts recording input and service responses.
    Record {
        /// Node keys whose text is recorded as `[redacted]`.
        #[arg(long)]
        redact: Vec<String>,
    },
    /// Stops recording and writes the recording.
    Stop {
        /// Where to write it.
        #[arg(long, default_value = "recording.json")]
        out: PathBuf,
    },
    /// Turns a recording into a regression test on the headless backend.
    ToTest {
        /// The recording.
        recording: PathBuf,
        /// The expression creating the root component
        /// (`my_app::Root::new(())`).
        #[arg(long)]
        launch: String,
        /// The test's name.
        #[arg(long, default_value = "replays_the_recording")]
        name: String,
        /// Where to write it (standard output if omitted).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// The overlay's modes, and off.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OverlayChoice {
    /// Node rectangles and keys.
    Layout,
    /// The most recent events' targets.
    Events,
    /// Each change's cost.
    FrameCost,
    /// Hide it.
    Off,
}

fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> Error {
    let what = what.into();
    move |cause| Error::Io { what, cause }
}

/// The endpoint to talk to.
fn endpoint(target: &Target) -> Result<Endpoint> {
    if let (Some(addr), Some(token)) = (target.addr, &target.token) {
        return Ok(Endpoint { addr, token: token.clone(), pid: 0 });
    }
    if target.android {
        return android_endpoint();
    }
    let directory = endpoint_directory();
    let read = |path: &Path| -> Option<(std::time::SystemTime, Endpoint)> {
        let modified = std::fs::metadata(path).and_then(|meta| meta.modified()).ok()?;
        let endpoint = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        Some((modified, endpoint))
    };
    let mut found: Vec<(std::time::SystemTime, Endpoint)> = std::fs::read_dir(&directory)
        .map(|entries| entries.flatten().filter_map(|entry| read(&entry.path())).collect())
        .unwrap_or_default();
    if let Some(pid) = target.pid {
        found.retain(|(_, endpoint)| endpoint.pid == pid);
    }
    found.sort_by_key(|(modified, _)| *modified);
    found.pop().map(|(_, endpoint)| endpoint).ok_or_else(|| {
        Error::Usage(format!(
            "no inspectable application found in {} — start it with RUSTNATIVE_INSPECT=1, or pass \
             --addr and --token",
            directory.display()
        ))
    })
}

/// The endpoint of the project's application on the attached Android
/// device: the device's inspection port (`rustnative_android`'s
/// `inspect::PORT`) forwarded to the same port here.
pub(crate) fn android_endpoint() -> Result<Endpoint> {
    const PORT: u16 = 7920;
    let here = std::env::current_dir().map_err(io("find the current directory"))?;
    let project = crate::project::Project::find(&here)?;
    let package = crate::package::android::application_id(&project.config);
    let toolchain = crate::package::android::toolchain()?;
    let adb = |arguments: &[&str]| crate::package::android::adb(&toolchain, arguments);
    let port = format!("tcp:{PORT}");
    adb(&["forward", &port, &port])?;
    let json = adb(&["shell", "run-as", &package, "cat", "files/rustnative/inspect.json"])?;
    let mut endpoint: Endpoint = serde_json::from_str(json.trim()).map_err(|_| {
        Error::Usage(format!(
            "{package} is not inspectable: start it with `rustnative run android --inspect` (a debug build)"
        ))
    })?;
    endpoint.addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    Ok(endpoint)
}

/// Runs `question` against the application `target` names.
pub fn run(target: &Target, question: Question) -> Result<()> {
    if let Question::ToTest { recording, launch, name, out } = question {
        let text = std::fs::read_to_string(&recording)
            .map_err(io(format!("read {}", recording.display())))?;
        let recording: Recording = serde_json::from_str(&text).map_err(|error| {
            Error::Usage(format!("{} is not a recording: {error}", recording.display()))
        })?;
        let generated = recording.to_test(&name, &launch);
        if let Some(path) = out {
            return std::fs::write(&path, generated)
                .map_err(io(format!("write {}", path.display())));
        }
        print!("{generated}");
        return Ok(());
    }
    let request = match &question {
        Question::Hello => Request::Hello,
        Question::Tree { window } => Request::Tree { window: *window },
        Question::Components { window } => Request::Components { window: *window },
        Question::Realized { window } => Request::Realized { window: *window },
        Question::State { path } => Request::State { path: path.clone(), window: None },
        Question::Set { path, field, value } => Request::SetState {
            path: path.clone(),
            field: field.clone(),
            value: serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.clone())),
            window: None,
        },
        Question::Explain { node } => Request::ExplainLayout { node: node.clone(), window: None },
        Question::Style { node } => Request::ExplainStyle { node: node.clone(), window: None },
        Question::Trace { since } => Request::Trace { since: *since },
        Question::Tasks => Request::Tasks { window: None },
        Question::Lifetimes => Request::Lifetimes,
        Question::Stores => Request::Stores,
        Question::Caps => Request::Capabilities,
        Question::Mappers => Request::Mappers,
        Question::History { component } => Request::History { component: component.clone() },
        Question::Overlay { mode } => Request::Overlay {
            mode: match mode {
                OverlayChoice::Layout => Some(OverlayMode::Layout),
                OverlayChoice::Events => Some(OverlayMode::Events),
                OverlayChoice::FrameCost => Some(OverlayMode::FrameCost),
                OverlayChoice::Off => None,
            },
        },
        Question::Record { redact } => Request::StartRecording { redact: redact.clone() },
        Question::Stop { .. } => Request::StopRecording,
        Question::ToTest { .. } => unreachable!("answered above"),
    };
    let endpoint = endpoint(target)?;
    let reply = send_request(&endpoint, &request)
        .map_err(io(format!("reach the application at {}", endpoint.addr)))?;
    let value = match reply {
        Reply::Ok(value) => value,
        Reply::Error(message) => {
            return Err(Error::Usage(format!("the application says: {message}")));
        }
    };
    if let Question::Stop { out } = &question {
        let text = serde_json::to_string_pretty(&value).unwrap_or_default();
        std::fs::write(out, text).map_err(io(format!("write {}", out.display())))?;
        println!(
            "recorded {} inputs to {}",
            value["inputs"].as_array().map_or(0, Vec::len),
            out.display()
        );
        return Ok(());
    }
    if target.json {
        println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
    } else {
        print!("{}", render(&question, &value));
    }
    Ok(())
}

/// The answer as text.
fn render(question: &Question, value: &Value) -> String {
    let text = |value: &Value| value.as_str().map_or_else(|| value.to_string(), str::to_owned);
    let mut out = String::new();
    let mut line = |line: String| {
        out.push_str(&line);
        out.push('\n');
    };
    match question {
        Question::Hello => line(format!(
            "protocol {} (client {PROTOCOL_VERSION}), backend {}, windows {}",
            value["version"],
            text(&value["backend"]),
            value["windows"]
        )),
        Question::Tree { .. } => tree(value, 0, &mut line),
        Question::Components { .. } => {
            for component in value.as_array().into_iter().flatten() {
                line(format!(
                    "{}  {}  tasks={}{}",
                    text(&component["path"]),
                    text(&component["type_name"]),
                    component["tasks"],
                    if component["state"].is_null() {
                        String::new()
                    } else {
                        format!("  {}", component["state"])
                    }
                ));
            }
        }
        Question::Realized { .. } => {
            for object in value.as_array().into_iter().flatten() {
                line(format!(
                    "{:<16} {:<28} {} {}",
                    object["key"].as_str().unwrap_or("-"),
                    text(&object["host_type"]),
                    object["handle"].as_str().unwrap_or("-"),
                    object["rect"]
                ));
            }
        }
        Question::Explain { .. } => {
            line(format!("{} at {}", value["key"].as_str().unwrap_or("?"), value["rect"]));
            for reason in value["reasons"].as_array().into_iter().flatten() {
                line(format!("  - {}", text(reason)));
            }
        }
        Question::Style { .. } => {
            line(format!("{} ({})", value["key"].as_str().unwrap_or("?"), text(&value["kind"])));
            for source in value["sources"].as_array().into_iter().flatten() {
                let origin = &source["origin"];
                let from = match origin["level"].as_str() {
                    Some("class") => format!("class `{}`", text(&origin["class"])),
                    Some("declaration") => format!("declaration `{}`", text(&origin["text"])),
                    Some("typed_override") => "typed override".to_owned(),
                    Some("component_default") => format!("{} default", text(&origin["kind"])),
                    _ => origin.to_string(),
                };
                line(format!(
                    "  {}{} = {}{}  <- {from}{}",
                    source["condition"].as_str().unwrap_or(""),
                    text(&source["property"]),
                    text(&source["value"]),
                    source["resolved"].as_str().map(|r| format!(" ({r})")).unwrap_or_default(),
                    if source["applies"] == Value::Bool(true) { "" } else { "  [not now]" }
                ));
            }
        }
        Question::Trace { .. } => {
            for entry in value.as_array().into_iter().flatten() {
                let kind = &entry["kind"];
                line(format!(
                    "#{} {}us {} {}",
                    entry["seq"],
                    entry["micros"],
                    text(&kind["kind"]),
                    kind.get("event")
                        .or_else(|| kind.get("component"))
                        .map(text)
                        .unwrap_or_default()
                ));
                let pass = &kind["pass"];
                for render in pass["rendered"].as_array().into_iter().flatten() {
                    line(format!(
                        "    rendered {} ({})",
                        text(&render["component"]),
                        text(&render["cause"])
                    ));
                }
                for skipped in pass["skipped"].as_array().into_iter().flatten() {
                    line(format!("    skipped  {} (nothing it depends on changed)", text(skipped)));
                }
            }
        }
        Question::Caps => {
            line(format!("backend {}", text(&value["backend"])));
            line(format!("advertised: {}", value["advertised"]));
            for refusal in value["refused"].as_array().into_iter().flatten() {
                line(format!("refused {}: {}", text(&refusal["what"]), text(&refusal["why"])));
            }
            for row in value["style"].as_array().into_iter().flatten() {
                line(format!("style {}: {}", text(&row["what"]), text(&row["why"])));
            }
        }
        _ => line(serde_json::to_string_pretty(value).unwrap_or_default()),
    }
    out
}

fn tree(node: &Value, depth: usize, line: &mut impl FnMut(String)) {
    let classes: Vec<String> = node["classes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|class| format!(" .{}", class.as_str().unwrap_or_default()))
        .collect();
    line(format!(
        "{}{} {}{}  [{}]{}{}",
        "  ".repeat(depth),
        node["kind"].as_str().unwrap_or("?"),
        node["key"].as_str().unwrap_or("-"),
        node["text"].as_str().map(|text| format!(" {text:?}")).unwrap_or_default(),
        node["component"].as_str().unwrap_or_default(),
        classes.concat(),
        if node["hidden"] == Value::Bool(true) { " (hidden)" } else { "" }
    ));
    for child in node["children"].as_array().into_iter().flatten() {
        tree(child, depth + 1, line);
    }
}
