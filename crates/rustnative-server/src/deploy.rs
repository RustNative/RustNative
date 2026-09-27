//! Deployment (`PLAN.md` Milestone 50): the adapter contract, a long-lived
//! local deployment with immutable revisions and percentage traffic
//! splitting, and infrastructure descriptions for containers.
//!
//! A [`Revision`] is one immutable build of the application, listening on
//! its own address. The [`TrafficSplitter`] is a small reverse proxy in
//! front of the revisions: it sends each client to a revision by weight
//! (the same client to the same revision, by a hash of its address), lets
//! a named revision be previewed with `x-revision`, and moves traffic —
//! promote, roll back — without restarting anything. [`LocalAdapter`]
//! drives it for a [`Target`] — a long-lived server, a static host, a
//! function runtime, or an edge host (Web milestone K), each revision being
//! the server, `rustnative serve static`, `serve lambda`, or `serve wagi`
//! on its own port. [`container`] writes the descriptions a container
//! platform takes, and [`serverless`] the ones a function runtime (an AWS
//! SAM template) and an edge host (a Spin manifest) take.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use serde::{Deserialize, Serialize};

use crate::response::{IntoResponse, Response, ServerError};

/// What a host allows each request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostLimits {
    /// The longest a request may run.
    pub deadline: Option<Duration>,
    /// Memory, in megabytes.
    pub memory_mb: Option<u64>,
    /// The file system: `read-write`, `read-only`, or `none`.
    pub filesystem: String,
    /// The largest request payload.
    pub payload_bytes: Option<u64>,
}

/// Where a deployment runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// A long-lived server.
    #[default]
    Server,
    /// Static files on a static host or content network.
    Static,
    /// A function per request, on a function runtime (AWS Lambda's API).
    Function,
    /// A WebAssembly module per request, on an edge host (WAGI).
    Edge,
}

impl Target {
    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Server => "local server",
            Self::Static => "static host",
            Self::Function => "function runtime",
            Self::Edge => "edge host",
        }
    }

    /// What its host allows each request: the defaults of the emulators,
    /// which follow the common providers'.
    #[must_use]
    pub fn limits(self) -> HostLimits {
        match self {
            Self::Server => HostLimits {
                deadline: None,
                memory_mb: None,
                filesystem: "read-write".into(),
                payload_bytes: Some(1024 * 1024),
            },
            Self::Static => HostLimits {
                deadline: None,
                memory_mb: None,
                filesystem: "none".into(),
                payload_bytes: Some(0),
            },
            Self::Function => HostLimits {
                deadline: Some(Duration::from_secs(10)),
                memory_mb: Some(128),
                // `/tmp`, per instance.
                filesystem: "read-write".into(),
                payload_bytes: Some(6 * 1024 * 1024),
            },
            Self::Edge => HostLimits {
                deadline: Some(Duration::from_secs(30)),
                memory_mb: Some(128),
                filesystem: "none".into(),
                payload_bytes: Some(6 * 1024 * 1024),
            },
        }
    }
}

/// One immutable build, serving at an address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    /// Its name (`r3`, a commit hash).
    pub name: String,
    /// Where it listens.
    pub address: SocketAddr,
}

/// Where traffic goes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeploymentStatus {
    /// Every revision.
    pub revisions: Vec<Revision>,
    /// The share of traffic each named revision takes, in percent.
    pub weights: BTreeMap<String, u8>,
    /// The revision to roll back to.
    pub previous: Option<String>,
}

/// What a deployment target does.
#[async_trait::async_trait]
pub trait DeploymentAdapter: Send + Sync {
    /// The target's name.
    fn name(&self) -> &str;
    /// What the target allows each request.
    fn limits(&self) -> HostLimits;
    /// Adds a revision, taking no traffic (a preview).
    ///
    /// # Errors
    ///
    /// The target refused.
    async fn deploy(&self, revision: Revision) -> Result<(), String>;
    /// Sends `percent` of traffic to `revision`, the rest to the current one.
    ///
    /// # Errors
    ///
    /// No such revision.
    async fn promote(&self, revision: &str, percent: u8) -> Result<(), String>;
    /// Sends all traffic back to the previous revision.
    ///
    /// # Errors
    ///
    /// There is none.
    async fn rollback(&self) -> Result<(), String>;
    /// Where traffic goes.
    fn status(&self) -> DeploymentStatus;
}

/// The reverse proxy in front of the revisions.
#[derive(Clone, Default)]
pub struct TrafficSplitter {
    status: Arc<Mutex<DeploymentStatus>>,
}

fn bucket(key: &str) -> u8 {
    // FNV-1a: stable across processes, so a client keeps its revision.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    u8::try_from(hash % 100).unwrap_or(0)
}

impl TrafficSplitter {
    /// A splitter with no revisions.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Where traffic goes.
    #[must_use]
    pub fn status(&self) -> DeploymentStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn update(
        &self,
        change: impl FnOnce(&mut DeploymentStatus) -> Result<(), String>,
    ) -> Result<(), String> {
        change(&mut self.status.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The revision a request goes to: the previewed one if it names one
    /// (`x-revision`), else by the client's bucket and the weights.
    #[must_use]
    pub fn choose(&self, client: &str, preview: Option<&str>) -> Option<Revision> {
        let status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(name) = preview {
            return status.revisions.iter().find(|revision| revision.name == name).cloned();
        }
        let bucket = bucket(client);
        let mut floor = 0u16;
        for (name, weight) in &status.weights {
            floor += u16::from(*weight);
            if u16::from(bucket) < floor {
                return status.revisions.iter().find(|revision| &revision.name == name).cloned();
            }
        }
        None
    }

    /// Forwards `request` to the chosen revision, telling it the client's
    /// address in `x-forwarded-for` (replacing any the client sent).
    pub async fn forward(&self, mut request: http::Request<Bytes>, client: SocketAddr) -> Response {
        if let Ok(value) = http::HeaderValue::from_str(&client.ip().to_string()) {
            request.headers_mut().insert("x-forwarded-for", value);
        }
        let preview = request
            .headers()
            .get("x-revision")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let Some(revision) = self.choose(&client.ip().to_string(), preview.as_deref()) else {
            return ServerError::new(
                http::StatusCode::SERVICE_UNAVAILABLE,
                "No revision is serving",
            )
            .into_response();
        };
        match forward_to(revision.address, request).await {
            Ok(mut response) => {
                if let Ok(value) = http::HeaderValue::from_str(&revision.name) {
                    response.headers_mut().insert("x-served-by", value);
                }
                response
            }
            Err(error) => ServerError::new(http::StatusCode::BAD_GATEWAY, error).into_response(),
        }
    }

    /// Serves the proxy on `listener`.
    pub async fn serve(self, listener: tokio::net::TcpListener) {
        while let Ok((stream, peer)) = listener.accept().await {
            let splitter = self.clone();
            tokio::spawn(async move {
                let io = hyper_util::rt::TokioIo::new(stream);
                let service = hyper::service::service_fn(
                    move |request: http::Request<hyper::body::Incoming>| {
                        let splitter = splitter.clone();
                        async move {
                            let (parts, body) = request.into_parts();
                            let body = body
                                .collect()
                                .await
                                .map(http_body_util::Collected::to_bytes)
                                .unwrap_or_default();
                            let response = splitter
                                .forward(http::Request::from_parts(parts, body), peer)
                                .await;
                            Ok::<_, std::convert::Infallible>(response.map(Full::new))
                        }
                    },
                );
                let _ =
                    hyper::server::conn::http1::Builder::new().serve_connection(io, service).await;
            });
        }
    }
}

async fn forward_to(
    address: SocketAddr,
    request: http::Request<Bytes>,
) -> Result<Response, String> {
    let stream =
        tokio::net::TcpStream::connect(address).await.map_err(|error| error.to_string())?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
            .await
            .map_err(|error| error.to_string())?;
    tokio::spawn(connection);
    let (parts, body) = request.into_parts();
    let response = sender
        .send_request(http::Request::from_parts(parts, Full::new(body)))
        .await
        .map_err(|error| error.to_string())?;
    let (parts, body) = response.into_parts();
    let body = body.collect().await.map_err(|error| error.to_string())?.to_bytes();
    Ok(http::Response::from_parts(parts, body))
}

/// A deployment on this machine: revisions are processes on local ports
/// behind a [`TrafficSplitter`] — servers, or, for the other targets, the
/// emulators serving each revision's artifact.
#[derive(Clone, Default)]
pub struct LocalAdapter {
    splitter: TrafficSplitter,
    target: Target,
}

impl LocalAdapter {
    /// An adapter over `splitter`, for a long-lived server.
    #[must_use]
    pub fn new(splitter: TrafficSplitter) -> Self {
        Self { splitter, target: Target::Server }
    }

    /// An adapter for `target`.
    #[must_use]
    pub fn for_target(target: Target) -> Self {
        Self { splitter: TrafficSplitter::new(), target }
    }

    /// Its target.
    #[must_use]
    pub const fn target(&self) -> Target {
        self.target
    }
}

#[async_trait::async_trait]
impl DeploymentAdapter for LocalAdapter {
    fn name(&self) -> &'static str {
        self.target.name()
    }

    fn limits(&self) -> HostLimits {
        self.target.limits()
    }

    async fn deploy(&self, revision: Revision) -> Result<(), String> {
        self.splitter.update(|status| {
            if status.revisions.iter().any(|existing| existing.name == revision.name) {
                return Err(format!("revision {} exists; revisions are immutable", revision.name));
            }
            if status.weights.is_empty() {
                // The first revision takes all the traffic.
                status.weights.insert(revision.name.clone(), 100);
            }
            status.revisions.push(revision);
            Ok(())
        })
    }

    async fn promote(&self, revision: &str, percent: u8) -> Result<(), String> {
        self.splitter.update(|status| {
            if !status.revisions.iter().any(|existing| existing.name == revision) {
                return Err(format!("no revision {revision}"));
            }
            let current = status
                .weights
                .iter()
                .max_by_key(|(_, weight)| **weight)
                .map(|(name, _)| name.clone())
                .filter(|name| name != revision);
            let percent = percent.min(100);
            status.weights.clear();
            status.weights.insert(revision.to_owned(), percent);
            if let Some(current) = current {
                if percent < 100 {
                    status.weights.insert(current.clone(), 100 - percent);
                }
                status.previous = Some(current);
            }
            Ok(())
        })
    }

    async fn rollback(&self) -> Result<(), String> {
        self.splitter.update(|status| {
            let previous = status.previous.take().ok_or("there is no previous revision")?;
            status.weights.clear();
            status.weights.insert(previous, 100);
            Ok(())
        })
    }

    fn status(&self) -> DeploymentStatus {
        self.splitter.status()
    }
}

#[derive(Deserialize)]
struct Promotion {
    revision: String,
    percent: u8,
}

impl LocalAdapter {
    /// The control API `rustnative deploy local` speaks, to serve on a
    /// loopback address only: `GET /status`, `GET /target` (its name and
    /// limits), `POST /revisions` (a [`Revision`]), `POST /promote`
    /// (`{ revision, percent }`), and `POST /rollback`.
    #[must_use]
    pub fn control(&self) -> crate::ServerApp {
        let (status, deploying, promoting, rolling) =
            (self.clone(), self.clone(), self.clone(), self.clone());
        let target = self.target;
        crate::ServerApp::new()
            .security(crate::Security { csrf: false, hsts: false, ..crate::Security::default() })
            .route(
                "/target",
                crate::get(move || async move {
                    crate::Json(serde_json::json!({
                        "target": target,
                        "name": target.name(),
                        "limits": target.limits(),
                    }))
                })
                .public(),
            )
            .route(
                "/status",
                crate::get(move || {
                    let adapter = status.clone();
                    async move { crate::Json(adapter.status()) }
                })
                .public(),
            )
            .route(
                "/revisions",
                crate::post(move |crate::Json(revision): crate::Json<Revision>| {
                    let adapter = deploying.clone();
                    async move {
                        adapter
                            .deploy(revision)
                            .await
                            .map(|()| crate::Json(adapter.status()))
                            .map_err(ServerError::bad_request)
                    }
                })
                .public(),
            )
            .route(
                "/promote",
                crate::post(move |crate::Json(promotion): crate::Json<Promotion>| {
                    let adapter = promoting.clone();
                    async move {
                        adapter
                            .promote(&promotion.revision, promotion.percent)
                            .await
                            .map(|()| crate::Json(adapter.status()))
                            .map_err(ServerError::bad_request)
                    }
                })
                .public(),
            )
            .route(
                "/rollback",
                crate::post(move || {
                    let adapter = rolling.clone();
                    async move {
                        adapter
                            .rollback()
                            .await
                            .map(|()| crate::Json(adapter.status()))
                            .map_err(ServerError::bad_request)
                    }
                })
                .public(),
            )
    }

    /// The splitter this adapter drives.
    #[must_use]
    pub fn splitter(&self) -> TrafficSplitter {
        self.splitter.clone()
    }
}

/// Infrastructure descriptions for container platforms.
pub mod container {
    use std::fmt::Write as _;

    /// What the descriptions describe.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Service {
        /// The application's name.
        pub name: String,
        /// Its binary's name.
        pub binary: String,
        /// The port it listens on.
        pub port: u16,
        /// Environment variables it needs (names only: values are the
        /// platform's secrets).
        pub environment: Vec<String>,
        /// Declared resources it binds (least privilege: only these).
        pub resources: Vec<String>,
    }

    /// A Dockerfile: the single statically linked artifact on a minimal
    /// base, running as an unprivileged user.
    #[must_use]
    pub fn dockerfile(service: &Service) -> String {
        format!(
            "# Generated by `rustnative deploy export container`.\n\
             FROM gcr.io/distroless/cc-debian12:nonroot\n\
             COPY target/release/{binary} /app/{binary}\n\
             USER nonroot\n\
             EXPOSE {port}\n\
             ENTRYPOINT [\"/app/{binary}\"]\n",
            binary = service.binary,
            port = service.port
        )
    }

    /// A Compose file.
    #[must_use]
    pub fn compose(service: &Service) -> String {
        let mut environment = String::new();
        for name in &service.environment {
            let _ = writeln!(environment, "      {name}: ${{{name}}}");
        }
        format!(
            "# Generated by `rustnative deploy export compose`.\nservices:\n  {name}:\n    build: .\n    ports:\n      - \"{port}:{port}\"\n    read_only: true\n    environment:\n{environment}",
            name = service.name,
            port = service.port
        )
    }

    /// A Kubernetes Deployment and Service.
    #[must_use]
    pub fn kubernetes(service: &Service) -> String {
        let mut environment = String::new();
        for name in &service.environment {
            let _ = writeln!(
                environment,
                "            - name: {name}\n              valueFrom:\n                secretKeyRef: {{ name: {app}-secrets, key: {name} }}",
                app = service.name
            );
        }
        format!(
            "# Generated by `rustnative deploy export kubernetes`.\n\
             apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: {name}\nspec:\n  replicas: 2\n  selector:\n    matchLabels: {{ app: {name} }}\n  template:\n    metadata:\n      labels: {{ app: {name} }}\n    spec:\n      securityContext: {{ runAsNonRoot: true }}\n      containers:\n        - name: {name}\n          image: {name}:latest\n          ports: [{{ containerPort: {port} }}]\n          readinessProbe: {{ httpGet: {{ path: /readyz, port: {port} }} }}\n          livenessProbe: {{ httpGet: {{ path: /healthz, port: {port} }} }}\n          securityContext: {{ readOnlyRootFilesystem: true, allowPrivilegeEscalation: false }}\n          env:\n{environment}---\napiVersion: v1\nkind: Service\nmetadata:\n  name: {name}\nspec:\n  selector: {{ app: {name} }}\n  ports: [{{ port: 80, targetPort: {port} }}]\n",
            name = service.name,
            port = service.port
        )
    }

    /// A systemd unit.
    #[must_use]
    pub fn systemd(service: &Service) -> String {
        format!(
            "# Generated by `rustnative deploy export systemd`.\n[Unit]\nDescription={name}\nAfter=network-online.target\n\n[Service]\nExecStart=/opt/{name}/{binary}\nDynamicUser=yes\nNoNewPrivileges=yes\nProtectSystem=strict\nRestart=on-failure\nEnvironmentFile=-/etc/{name}/environment\n\n[Install]\nWantedBy=multi-user.target\n",
            name = service.name,
            binary = service.binary
        )
    }
}

/// Descriptions for the serverless targets' providers.
pub mod serverless {
    use std::fmt::Write as _;

    /// A function, as a function runtime deploys it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Function {
        /// The application's name.
        pub name: String,
        /// Where the staged function is (`target/web/lambda`, holding
        /// `bootstrap`).
        pub code: String,
        /// Its memory, in megabytes.
        pub memory_mb: u64,
        /// Its timeout, in seconds.
        pub timeout_s: u64,
        /// Environment variables it needs (names only: values are the
        /// platform's secrets).
        pub environment: Vec<String>,
    }

    /// An AWS SAM template: the function on the `provided.al2023` runtime
    /// (the binary is `bootstrap`), behind an HTTP API, with a preview
    /// alias and gradual traffic shifting — the provider's own preview,
    /// promotion, and rollback.
    #[must_use]
    pub fn sam(function: &Function) -> String {
        let mut environment = String::new();
        for name in &function.environment {
            let _ = writeln!(environment, "          {name}: !Ref {}", parameter(name));
        }
        let mut parameters = String::new();
        for name in &function.environment {
            let _ =
                writeln!(parameters, "  {}:\n    Type: String\n    NoEcho: true", parameter(name));
        }
        let variables = if environment.is_empty() {
            String::new()
        } else {
            format!("      Environment:\n        Variables:\n{environment}")
        };
        format!(
            "# Generated by `rustnative deploy export sam`.\n\
             AWSTemplateFormatVersion: '2010-09-09'\n\
             Transform: AWS::Serverless-2016-10-31\n\
             Description: {name}\n\
             Parameters:\n{parameters_or_empty}\
             Resources:\n  \
               App:\n    \
                 Type: AWS::Serverless::Function\n    \
                 Properties:\n      \
                   CodeUri: {code}\n      \
                   Handler: bootstrap\n      \
                   Runtime: provided.al2023\n      \
                   Architectures: [x86_64]\n      \
                   MemorySize: {memory}\n      \
                   Timeout: {timeout}\n      \
                   AutoPublishAlias: live\n      \
                   DeploymentPreference:\n        \
                     Type: Canary10Percent5Minutes\n\
             {variables}      \
                   Events:\n        \
                     Http:\n          \
                       Type: HttpApi\n\
             Outputs:\n  \
               Url:\n    \
                 Value: !Sub 'https://${{ServerlessHttpApi}}.execute-api.${{AWS::Region}}.amazonaws.com/'\n",
            name = function.name,
            parameters_or_empty =
                if parameters.is_empty() { "  {}\n".to_owned() } else { parameters },
            code = function.code,
            memory = function.memory_mb,
            timeout = function.timeout_s,
        )
    }

    /// `NOTES_DATA_KEY` as a template parameter's name: `NotesDataKey`.
    fn parameter(name: &str) -> String {
        name.split('_')
            .map(|word| {
                let lower = word.to_ascii_lowercase();
                let mut letters = lower.chars();
                letters.next().map_or_else(String::new, |first| {
                    first.to_ascii_uppercase().to_string() + letters.as_str()
                })
            })
            .collect()
    }

    /// An edge module, as an edge host deploys it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct EdgeModule {
        /// The application's name.
        pub name: String,
        /// The module (`target/web/wagi/<name>.wasm`).
        pub source: String,
        /// The hosts it may send HTTP requests to (`https://data.example`).
        pub outbound: Vec<String>,
        /// Environment variables it needs (`name = "{{ variable }}"`, so
        /// values come from the platform's variables).
        pub environment: Vec<String>,
    }

    /// A Spin manifest: the module under the WAGI executor on every route.
    #[must_use]
    pub fn spin(module: &EdgeModule) -> String {
        let component = module.name.replace('_', "-");
        let mut variables = String::new();
        let mut environment = String::new();
        for name in &module.environment {
            let lower = name.to_ascii_lowercase();
            let _ = writeln!(variables, "{lower} = {{ required = true, secret = true }}");
            let _ = write!(environment, "{name} = \"{{{{ {lower} }}}}\", ");
        }
        let outbound =
            module.outbound.iter().map(|host| format!("\"{host}\"")).collect::<Vec<_>>().join(", ");
        format!(
            "# Generated by `rustnative deploy export spin`.\n\
             spin_manifest_version = 2\n\n\
             [application]\n\
             name = \"{name}\"\n\
             version = \"0.1.0\"\n\n\
             [variables]\n{variables}\n\
             [[trigger.http]]\n\
             route = \"/...\"\n\
             component = \"{component}\"\n\
             executor = {{ type = \"wagi\" }}\n\n\
             [component.{component}]\n\
             source = \"{source}\"\n\
             allowed_outbound_hosts = [{outbound}]\n\
             environment = {{ {environment} }}\n",
            name = module.name,
            source = module.source,
            environment = environment.trim_end_matches([',', ' ']),
        )
    }
}
