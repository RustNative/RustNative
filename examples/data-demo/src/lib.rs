//! A task board over a server (`PLAN.md` Milestone 47's example).
//!
//! - The task list is an infinite query: cached, loaded a page at a time,
//!   and shared with the summary above it, which asks for the same key, so
//!   one request serves both.
//! - Adding a task is an optimistic mutation: it appears at once and is
//!   confirmed (or rolled back) by the server.
//! - Offline, additions queue in the state store and are sent in order when
//!   the network returns.
//! - The weather widget fails — twice when it first renders, and whenever
//!   its "break" button is pressed. Its error boundary contains the
//!   failure: the rest of the board is untouched, the widget restarts after
//!   a backoff, and a failure it cannot outlast offers "Try again".
//!
//! The server is in-process ([`DemoServer`]), so the example runs anywhere,
//! with latency on a real executor and none on virtual time.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use rustnative_core::{
    Component, ComponentContext, Event, HttpRequest, HttpResponse, HttpService, Method, Node,
    NodeId, ServiceError, SupervisionPolicy,
};
use rustnative_data::{
    ConflictPolicy, Endpoint, HttpClient, Mutation, MutationError, Page, Pages, Query, QueryClient,
    QueryError, QueryKey, QueryState,
};
use serde::{Deserialize, Serialize};

/// Tasks per page.
pub const PAGE_SIZE: usize = 5;

/// One task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Its id; 0 until the server assigns one.
    pub id: u32,
    /// What to do.
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TaskPage {
    items: Vec<Task>,
    next: Option<u32>,
}

#[derive(Debug, Default)]
struct ServerState {
    tasks: Vec<Task>,
    requests: Vec<String>,
}

/// The in-process server: pages of tasks and task creation, with a switch
/// that makes it unreachable, and a count of the requests it received.
#[derive(Clone)]
pub struct DemoServer {
    state: Arc<Mutex<ServerState>>,
    reachable: Arc<AtomicBool>,
    latency: Duration,
    /// How many more times the weather widget fails when rendered.
    flaky_renders: Arc<AtomicU32>,
}

impl std::fmt::Debug for DemoServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DemoServer").finish_non_exhaustive()
    }
}

impl PartialEq for DemoServer {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl DemoServer {
    /// A server holding `count` tasks, answering after `latency` when run
    /// on a real executor.
    #[must_use]
    pub fn new(count: u32, latency: Duration) -> Self {
        let tasks = (1..=count).map(|id| Task { id, title: format!("Task {id}") }).collect();
        Self {
            state: Arc::new(Mutex::new(ServerState { tasks, requests: Vec::new() })),
            reachable: Arc::new(AtomicBool::new(true)),
            latency,
            flaky_renders: Arc::new(AtomicU32::new(2)),
        }
    }

    /// Makes the server reachable or not.
    pub fn set_reachable(&self, reachable: bool) {
        self.reachable.store(reachable, Ordering::SeqCst);
    }

    /// Whether it is reachable.
    #[must_use]
    pub fn is_reachable(&self) -> bool {
        self.reachable.load(Ordering::SeqCst)
    }

    /// Every request it has received, as `METHOD path`.
    #[must_use]
    pub fn requests(&self) -> Vec<String> {
        self.state.lock().requests.clone()
    }

    /// The titles of its tasks.
    #[must_use]
    pub fn titles(&self) -> Vec<String> {
        self.state.lock().tasks.iter().map(|task| task.title.clone()).collect()
    }

    fn respond(&self, request: &HttpRequest) -> Result<HttpResponse, ServiceError> {
        if !self.is_reachable() {
            return Err(ServiceError::new("the server is unreachable"));
        }
        let path = request.url().trim_start_matches("demo://server/").to_owned();
        let mut state = self.state.lock();
        state.requests.push(format!("{} {path}", request.method()));
        let json =
            |status, body: &dyn erased::Json| HttpResponse::new(status, vec![], body.bytes());
        match (request.method(), path.split_once('?')) {
            (Method::Get, Some(("tasks", query))) => {
                let page = query.trim_start_matches("page=").parse::<usize>().unwrap_or(0);
                let start = page * PAGE_SIZE;
                let items = state.tasks.iter().skip(start).take(PAGE_SIZE).cloned().collect();
                let next = (start + PAGE_SIZE < state.tasks.len())
                    .then(|| u32::try_from(page + 1).unwrap_or(u32::MAX));
                Ok(json(200, &TaskPage { items, next }))
            }
            (Method::Post, None) if path == "tasks" => {
                let mut task: Task = serde_json::from_slice(request.body_bytes())
                    .map_err(|error| ServiceError::new(error.to_string()))?;
                if task.title.trim().is_empty() {
                    return Ok(HttpResponse::new(422, vec![], b"a task needs a title".to_vec()));
                }
                task.id = u32::try_from(state.tasks.len() + 1).unwrap_or(u32::MAX);
                state.tasks.push(task.clone());
                Ok(json(201, &task))
            }
            _ => Ok(HttpResponse::new(404, vec![], Vec::new())),
        }
    }
}

mod erased {
    /// Serializes a response body, whatever its type.
    pub(crate) trait Json {
        fn bytes(&self) -> Vec<u8>;
    }

    impl<T: serde::Serialize> Json for T {
        fn bytes(&self) -> Vec<u8> {
            serde_json::to_vec(self).unwrap_or_default()
        }
    }
}

#[async_trait::async_trait]
impl HttpService for DemoServer {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ServiceError> {
        // Latency only where there is a real clock to wait on.
        if !self.latency.is_zero() && tokio::runtime::Handle::try_current().is_ok() {
            tokio::time::sleep(self.latency).await;
        }
        self.respond(&request)
    }
}

const TASKS: Endpoint<(), TaskPage> = Endpoint::get("tasks?page={page}");
const ADD: Endpoint<Task, Task> = Endpoint::post("tasks");

/// The task list's key.
#[must_use]
pub fn tasks_key() -> QueryKey {
    QueryKey::from(["tasks"])
}

/// The task list: an infinite query, a page at a time.
#[must_use]
pub fn tasks_query(http: HttpClient) -> Query<Pages<Task, u32>> {
    Query::infinite(tasks_key(), move |cursor: Option<u32>| {
        let http = http.clone();
        async move {
            let page = cursor.unwrap_or(0).to_string();
            let page =
                TASKS.call(&http, &[("page", &page)], None).await.map_err(QueryError::from)?;
            Ok(Page { items: page.items, next: page.next })
        }
    })
    .retries(2, Duration::from_millis(200))
    .stale_time(Duration::from_secs(30))
}

/// The client the board uses: its offline queue kept in `store`, and the
/// "add" mutation sent through `http`.
#[must_use]
pub fn make_client(
    http: &HttpClient,
    store: Option<Arc<dyn rustnative_core::StateStore>>,
) -> QueryClient {
    let client = QueryClient::new();
    let client = match store {
        Some(store) => client.with_offline_queue(store, ConflictPolicy::ServerWins),
        None => client,
    };
    let http = http.clone();
    client.register_mutation("add", move |request| {
        let http = http.clone();
        async move {
            let task: Task = serde_json::from_value(request.payload)
                .map_err(|error| MutationError::Rejected(error.to_string()))?;
            match ADD.call(&http, &[], Some(&task)).await {
                Ok(_) => Ok(()),
                Err(error) if error.to_string().contains("unreachable") => {
                    Err(MutationError::Unreachable(error.to_string()))
                }
                Err(error) => Err(MutationError::Rejected(error.to_string())),
            }
        }
    });
    client
}

fn clicked(event: &Event, key: &str) -> bool {
    matches!(event, Event::Click { target } if *target == NodeId::from_key(key))
}

/// The board: toolbar, status, summary, list, and weather widget.
pub struct Board {
    server: DemoServer,
    client: Option<QueryClient>,
    added: u32,
}

impl Component for Board {
    type Props = DemoServer;
    type Message = ();

    fn new(server: DemoServer) -> Self {
        Self { server, client: None, added: 0 }
    }
    fn props(&self) -> &DemoServer {
        &self.server
    }
    fn set_props(&mut self, server: DemoServer) {
        self.server = server;
    }
    fn view(&self) -> Node {
        Node::column("board", [])
    }

    fn update(&mut self, event: Event) {
        let Some(client) = &self.client else { return };
        if clicked(&event, "add") {
            self.added += 1;
            let task = Task { id: 0, title: format!("New task {}", self.added) };
            let shown = task.clone();
            client.mutate(
                Mutation::new("add", &task)
                    .optimistic::<Pages<Task, u32>>(tasks_key(), move |pages| {
                        pages.items.push(shown); // where the server will put it
                    })
                    .invalidates(tasks_key()),
            );
        } else if clicked(&event, "load-more") {
            client.fetch_next_page(&tasks_key());
        } else if clicked(&event, "network") {
            let online = !client.is_online();
            self.server.set_reachable(online);
            client.set_online(online);
        }
    }

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let http = HttpClient::new(
            context.services().http().cloned().unwrap_or_else(|| Arc::new(self.server.clone())),
            "demo://server/",
        );
        let store = context.services().state_store().cloned();
        let client = context.provide_scoped_with(|| make_client(&http, store));
        client.bind(context.background());
        context.provide_scoped(http);
        self.client = Some(client.clone());

        let status = context.select(client.store(), rustnative_data::Cache::mutations);
        let loading_more = context.select(client.store(), |cache| cache.loading_more(&tasks_key()));
        let status_text = format!(
            "{} · {} sending · {} queued · {} rejected",
            if status.online { "Online" } else { "Offline" },
            status.pending,
            status.queued,
            status.rejected
        );
        Node::column(
            "board",
            [
                Node::row(
                    "toolbar",
                    [
                        Node::button("add", "Add task"),
                        Node::button(
                            "load-more",
                            if loading_more { "Loading…" } else { "Load more" },
                        ),
                        Node::button(
                            "network",
                            if status.online { "Go offline" } else { "Go online" },
                        ),
                    ],
                ),
                Node::label("status", status_text),
                context.child::<Summary>("summary"),
                context.child::<TaskList>("list"),
                context.boundary::<Weather>(
                    "weather",
                    self.server.clone(),
                    SupervisionPolicy::RestartWithBackoff {
                        initial: Duration::from_millis(200),
                        max: Duration::from_secs(2),
                        attempts: 3,
                    },
                    |failure| {
                        Node::column(
                            "weather-failed",
                            [
                                Node::label(
                                    "weather-error",
                                    format!("The weather failed: {}", failure.message),
                                ),
                                Node::button("retry", "Try again"),
                            ],
                        )
                    },
                ),
            ],
        )
    }
}

/// How many tasks are loaded — the same query as the list, so no second
/// request.
pub struct Summary;

impl Component for Summary {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("summary", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let (Some(client), Some(http)) =
            (context.scoped::<QueryClient>(), context.scoped::<HttpClient>())
        else {
            return self.view();
        };
        let text = match client.use_query(context, tasks_query(http)) {
            QueryState::Success(pages) => format!("{} tasks", pages.items.len()),
            QueryState::Refreshing(pages) => format!("{} tasks (refreshing)", pages.items.len()),
            QueryState::Loading => "Loading…".to_owned(),
            QueryState::Empty => "No tasks".to_owned(),
            QueryState::Failure(error) => format!("Could not load: {error}"),
        };
        Node::label("summary", text)
    }
}

/// The tasks.
pub struct TaskList;

impl Component for TaskList {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("tasks", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let (Some(client), Some(http)) =
            (context.scoped::<QueryClient>(), context.scoped::<HttpClient>())
        else {
            return self.view();
        };
        let rows = match client.use_query(context, tasks_query(http)) {
            QueryState::Success(pages) | QueryState::Refreshing(pages) => pages
                .items
                .iter()
                .enumerate()
                .map(|(index, task)| {
                    let mark = if task.id == 0 { " (saving)" } else { "" };
                    Node::label(format!("task-{index}"), format!("{}{mark}", task.title))
                })
                .collect(),
            _ => Vec::new(),
        };
        Node::column("tasks", rows)
    }
}

/// A widget that fails: when first rendered (twice), and when broken.
pub struct Weather {
    server: DemoServer,
}

impl Component for Weather {
    type Props = DemoServer;
    type Message = ();
    fn new(server: DemoServer) -> Self {
        Self { server }
    }
    fn props(&self) -> &DemoServer {
        &self.server
    }
    fn set_props(&mut self, server: DemoServer) {
        self.server = server;
    }
    fn view(&self) -> Node {
        let failing = self
            .server
            .flaky_renders
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
            .is_ok();
        assert!(!failing, "the forecast service sent garbage");
        Node::row(
            "weather",
            [Node::label("forecast", "Sunny, 21°C"), Node::button("break", "Break the widget")],
        )
    }
    fn update(&mut self, event: Event) {
        assert!(!clicked(&event, "break"), "the widget was broken on purpose");
    }
}
