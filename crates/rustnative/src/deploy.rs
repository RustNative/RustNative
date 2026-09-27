//! `rustnative deploy` and `rustnative update` (`PLAN.md` Milestone 50).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use rustnative_server::deploy::container::{self, Service};
use rustnative_server::deploy::serverless;

use crate::error::{Error, Result};
use crate::project::Project;

/// `rustnative deploy`.
#[derive(Debug, clap::Subcommand)]
pub enum DeployCommand {
    /// Write infrastructure descriptions to `deploy/`.
    Export {
        /// `container`, `compose`, `kubernetes`, `systemd`, `sam` (a
        /// function), `spin` (an edge module), or `all`.
        target: String,
        /// The port the server listens on.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Build the container image too (needs Docker).
        #[arg(long)]
        build: bool,
    },
    /// Drive a local long-lived deployment.
    Local {
        /// What to do.
        #[command(subcommand)]
        action: LocalAction,
    },
}

/// `rustnative deploy local`.
#[derive(Debug, clap::Subcommand)]
pub enum LocalAction {
    /// Run the traffic-splitting proxy (in the foreground).
    Start {
        /// What the revisions are: servers, or `serve static`, `serve
        /// lambda`, or `serve wagi` emulators, each on its own port.
        #[arg(long, value_enum, default_value = "server")]
        target: DeployTarget,
        /// Where clients connect.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// The loopback control port the other actions use.
        #[arg(long, default_value_t = 8081)]
        control: u16,
    },
    /// Add a revision (it takes no traffic until promoted, unless it is the first).
    Add {
        /// Its name.
        name: String,
        /// Where it listens (`127.0.0.1:9001`).
        address: String,
        /// The control port.
        #[arg(long, default_value_t = 8081)]
        control: u16,
    },
    /// Send a percentage of traffic to a revision.
    Promote {
        /// The revision.
        name: String,
        /// Its share.
        #[arg(long, default_value_t = 100)]
        percent: u8,
        /// The control port.
        #[arg(long, default_value_t = 8081)]
        control: u16,
    },
    /// Send all traffic back to the previous revision.
    Rollback {
        /// The control port.
        #[arg(long, default_value_t = 8081)]
        control: u16,
    },
    /// Where traffic goes.
    Status {
        /// The control port.
        #[arg(long, default_value_t = 8081)]
        control: u16,
    },
}

/// What a local deployment's revisions are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DeployTarget {
    /// Long-lived servers.
    Server,
    /// Static exports, each under `rustnative serve static`.
    Static,
    /// Functions, each under `rustnative serve lambda`.
    Function,
    /// Edge modules, each under `rustnative serve wagi`.
    Edge,
}

impl From<DeployTarget> for rustnative_server::deploy::Target {
    fn from(target: DeployTarget) -> Self {
        match target {
            DeployTarget::Server => Self::Server,
            DeployTarget::Static => Self::Static,
            DeployTarget::Function => Self::Function,
            DeployTarget::Edge => Self::Edge,
        }
    }
}

fn usage(error: impl std::fmt::Display) -> Error {
    Error::Usage(error.to_string())
}

fn io(what: String) -> impl FnOnce(std::io::Error) -> Error {
    move |cause| Error::Io { what, cause }
}

/// A plain HTTP/1.1 request to the loopback control API.
fn control(port: u16, method: &str, path: &str, body: &str) -> Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(io(format!("reach the deployment on port {port}")))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(io("send to the deployment".into()))?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(io("read the deployment's answer".into()))?;
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    if !head.starts_with("HTTP/1.1 2") {
        return Err(usage(format!("the deployment refused: {}", body.trim())));
    }
    Ok(body.to_owned())
}

/// Runs a `deploy` command.
///
/// # Errors
///
/// The project cannot be read, a file cannot be written, or the
/// deployment refused.
pub fn run(here: &Path, command: DeployCommand) -> Result<()> {
    match command {
        DeployCommand::Export { target, port, build } => {
            let project = Project::find(here)?;
            let service = Service {
                name: project.config.app.name.clone(),
                binary: project.config.app.name.clone(),
                port,
                environment: project
                    .config
                    .resources
                    .keys()
                    .map(|name| format!("RUSTNATIVE_RESOURCE_{}", name.to_uppercase()))
                    .collect(),
                resources: project.config.resources.keys().cloned().collect(),
            };
            let out = project.root.join("deploy");
            std::fs::create_dir_all(&out).map_err(io(format!("create {}", out.display())))?;
            let all = target == "all";
            let mut wrote = false;
            let mut emit = |name: &str, text: String| -> Result<()> {
                let path = out.join(name);
                std::fs::write(&path, text).map_err(io(format!("write {}", path.display())))?;
                println!("deploy: wrote {}", path.display());
                wrote = true;
                Ok(())
            };
            if all || target == "container" {
                emit("Dockerfile", container::dockerfile(&service))?;
            }
            if all || target == "compose" {
                emit("compose.yaml", container::compose(&service))?;
            }
            if all || target == "kubernetes" {
                emit("kubernetes.yaml", container::kubernetes(&service))?;
            }
            if all || target == "systemd" {
                emit(&format!("{}.service", service.name), container::systemd(&service))?;
            }
            if all || target == "sam" {
                let function = serverless::Function {
                    name: service.name.clone(),
                    code: "../target/web/lambda".into(),
                    memory_mb: 128,
                    timeout_s: 10,
                    environment: service.environment.clone(),
                };
                emit("template.yaml", serverless::sam(&function))?;
            }
            if all || target == "spin" {
                let module = serverless::EdgeModule {
                    name: service.name.clone(),
                    source: format!("../target/web/wagi/{}.wasm", service.binary),
                    outbound: Vec::new(),
                    environment: service.environment.clone(),
                };
                emit("spin.toml", serverless::spin(&module))?;
            }
            if !wrote {
                return Err(usage(format!(
                    "{target:?}: expected container, compose, kubernetes, systemd, sam, spin, or all"
                )));
            }
            if build {
                let docker = std::process::Command::new("docker").arg("--version").output();
                if docker.is_ok_and(|output| output.status.success()) {
                    let status = std::process::Command::new("docker")
                        .current_dir(&project.root)
                        .args(["build", "-f", "deploy/Dockerfile", "-t", &service.name, "."])
                        .status()
                        .map_err(io("run docker".into()))?;
                    if !status.success() {
                        return Err(usage("docker build failed"));
                    }
                } else {
                    println!(
                        "deploy: Docker is not installed; the Dockerfile is written, the image is not built"
                    );
                }
            }
            Ok(())
        }
        DeployCommand::Local { action } => match action {
            LocalAction::Start { target, port, control: control_port } => {
                let runtime =
                    tokio::runtime::Runtime::new().map_err(io("start the runtime".into()))?;
                runtime.block_on(async {
                    let adapter =
                        rustnative_server::deploy::LocalAdapter::for_target(target.into());
                    let proxy = tokio::net::TcpListener::bind(("0.0.0.0", port))
                        .await
                        .map_err(io(format!("listen on {port}")))?;
                    let controller = tokio::net::TcpListener::bind(("127.0.0.1", control_port))
                        .await
                        .map_err(io(format!("listen on {control_port}")))?;
                    println!(
                        "deploy: a {} on port {port}; control on 127.0.0.1:{control_port}",
                        adapter.target().name()
                    );
                    tokio::spawn(adapter.splitter().serve(proxy));
                    adapter
                        .control()
                        .into_service()
                        .serve(controller, std::future::pending())
                        .await
                        .map_err(io("serve".into()))
                })
            }
            LocalAction::Add { name, address, control: port } => {
                let address: std::net::SocketAddr = address.parse().map_err(usage)?;
                let body = serde_json::json!({ "name": name, "address": address }).to_string();
                println!("{}", control(port, "POST", "/revisions", &body)?);
                Ok(())
            }
            LocalAction::Promote { name, percent, control: port } => {
                let body = serde_json::json!({ "revision": name, "percent": percent }).to_string();
                println!("{}", control(port, "POST", "/promote", &body)?);
                Ok(())
            }
            LocalAction::Rollback { control: port } => {
                println!("{}", control(port, "POST", "/rollback", "")?);
                Ok(())
            }
            LocalAction::Status { control: port } => {
                println!("{}", control(port, "GET", "/status", "")?);
                Ok(())
            }
        },
    }
}

/// `rustnative update`.
#[derive(Debug, clap::Subcommand)]
pub enum UpdateCommand {
    /// Make a publisher key pair.
    Keygen {
        /// Where to write the secret key (keep it out of the repository).
        #[arg(long)]
        out: PathBuf,
    },
    /// Write a signed update manifest for a package.
    Manifest {
        /// The version offered.
        #[arg(long)]
        version: String,
        /// Where the package will be downloaded from.
        #[arg(long)]
        url: String,
        /// The package file.
        #[arg(long)]
        package: PathBuf,
        /// The share of installations offered it.
        #[arg(long, default_value_t = 100)]
        rollout: u8,
        /// The secret key file.
        #[arg(long)]
        key: PathBuf,
    },
}

fn secret(path: &Path) -> Result<[u8; 32]> {
    let text = std::fs::read_to_string(path).map_err(io(format!("read {}", path.display())))?;
    let bytes: Vec<u8> = (0..text.trim().len())
        .step_by(2)
        .filter_map(|at| u8::from_str_radix(text.trim().get(at..at + 2)?, 16).ok())
        .collect();
    bytes.try_into().map_err(|_| usage(format!("{} is not a 32-byte key", path.display())))
}

/// Runs an `update` command.
///
/// # Errors
///
/// A file cannot be read or written.
pub fn run_update(here: &Path, command: UpdateCommand) -> Result<()> {
    match command {
        UpdateCommand::Keygen { out } => {
            // A signing key comes from the operating system's
            // cryptographic random source, nothing weaker.
            let mut key = [0u8; 32];
            getrandom::getrandom(&mut key)
                .map_err(|error| usage(format!("no randomness: {error}")))?;
            let hex = hex(&key);
            std::fs::write(&out, hex).map_err(io(format!("write {}", out.display())))?;
            println!(
                "update: the secret key is in {}; keep it out of the repository",
                out.display()
            );
            println!(
                "update: put this in rustnative.toml:\n[update]\npublic-key = \"{}\"",
                update_key(&key)
            );
            Ok(())
        }
        UpdateCommand::Manifest { version, url, package, rollout, key } => {
            let project = Project::find(here)?;
            let bytes =
                std::fs::read(&package).map_err(io(format!("read {}", package.display())))?;
            let digest: String = sha2_hex(&bytes);
            let mut manifest = serde_json::json!({
                "app": project.config.app.id, "version": version, "url": url, "sha256": digest, "rollout": rollout,
                "payload": { "kind": "application" }, "signature": "",
            });
            let secret = secret(&key)?;
            let trusted = project.config.update.as_ref().map(|update| update.public_key.as_str());
            if trusted != Some(update_key(&secret).as_str()) {
                return Err(usage(format!(
                    "{} is not the key rustnative.toml's [update] public-key trusts; installed copies would refuse this update",
                    key.display()
                )));
            }
            let signature = sign(&secret, &manifest);
            manifest["signature"] = serde_json::Value::String(signature);
            println!("{}", serde_json::to_string_pretty(&manifest).unwrap_or_default());
            Ok(())
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn sha2_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(bytes))
}

fn update_key(secret: &[u8; 32]) -> String {
    hex(ed25519_dalek::SigningKey::from_bytes(secret).verifying_key().as_bytes())
}

/// Signs `manifest` (with an empty signature field) the way the updater
/// verifies it: Ed25519 over its JSON in field order.
fn sign(secret: &[u8; 32], manifest: &serde_json::Value) -> String {
    use ed25519_dalek::Signer;
    // The updater serializes its struct in declaration order: app,
    // version, url, sha256, rollout, payload, signature.
    let ordered = format!(
        r#"{{"app":{},"version":{},"url":{},"sha256":{},"rollout":{},"payload":{{"kind":"application"}},"signature":""}}"#,
        manifest["app"],
        manifest["version"],
        manifest["url"],
        manifest["sha256"],
        manifest["rollout"]
    );
    hex(&ed25519_dalek::SigningKey::from_bytes(secret).sign(ordered.as_bytes()).to_bytes())
}
