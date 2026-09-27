//! Pages (Web milestone H): a component tree rendered on the server for
//! one request, as a complete HTML document â€” whole, or streamed with its
//! slow parts following the first bytes.
//!
//! A [`Page`] is a head, a root component with its props, and how it
//! renders ([`Strategy`]). [`render`] runs it: a `ComponentTree` per
//! request, on the calling thread, with the request's services (the
//! [`RequestInfo`], the render's island registry) and a [`HostExecutor`] of
//! its own, so the render can wait for the data its components load â€” and
//! nothing process-wide is on the path. The renderer renders, runs the
//! request's tasks until none is left (or the page's budget runs out), and
//! renders again: a render may await, and is deterministic. When the
//! response is written, the tree and its executor are dropped, and every
//! task the request started is dropped with them.
//!
//! The document ships JavaScript only when the tree has an interactive
//! subtree (selective attachment, `W-IS-1`): each client component's root
//! is marked `data-rn-i`, the page data (`#rn-data`) lists them with their
//! state, and one module script â€” the runtime â€” attaches them.
//!
//! ```
//! use rustnative_core::{Component, Event, Node};
//! use rustnative_web::head::Head;
//! use rustnative_web::page::{Page, PageContext, render};
//! use rustnative_web::request::RequestInfo;
//!
//! struct About;
//! impl Component for About {
//!     type Props = ();
//!     type Message = ();
//!     fn new(_: ()) -> Self { Self }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, _: ()) {}
//!     fn view(&self) -> Node { Node::label("about", "Rust Native") }
//!     fn update(&mut self, _: Event) {}
//! }
//!
//! let page = Page::new::<About>(Head::new("About", "What Rust Native is."), ()).lang("en");
//! let rendered = render(page, &PageContext::new(Some(RequestInfo::get("/about"))));
//! assert!(rendered.html.starts_with("<!doctype html><html lang=\"en\">"));
//! assert!(rendered.html.contains("<span id=\"about\""));
//! assert!(!rendered.html.contains("<script"), "no interactive subtree, no JavaScript");
//! ```

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use rustnative_core::{
    Clock, Component, ComponentTree, HostExecutor, NodeId, Scheduler, Services, SystemClock, Theme,
};
use serde_json::{Map, Value, json};

use crate::client::{ClientModule, IslandKind, ServerRender};
use crate::css::StyleSheet;
use crate::dom::{Child, Element, Marks, Realizer};
use crate::head::Head;
use crate::html::{escape_into, json_for_script, render_into};
use crate::request::RequestInfo;

/// How a page renders (`W-MF-2`), declared per route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// Rendered once, with no request, and served as it is (a static
    /// export's pages; a server caches the document).
    Static,
    /// Rendered for each request, whole: the response starts when the
    /// render has settled.
    #[default]
    Server,
    /// Rendered for each request and streamed: the shell, with each
    /// [`crate::pending::pending`] boundary's fallback, is sent at once, and
    /// each boundary follows on the same response when its data arrives.
    Streamed,
    /// The server renders everything but the interactive subtrees, which
    /// the browser renders when it attaches them.
    ClientOnly,
}

type Build = Arc<dyn Fn(Services, Theme, Scheduler) -> ComponentTree + Send + Sync>;

/// A page: its head, its root component, and how it renders.
#[derive(Clone)]
pub struct Page {
    head: Head,
    lang: Option<String>,
    rtl: bool,
    strategy: Strategy,
    partial: bool,
    theme: Theme,
    budget: Duration,
    status: u16,
    build: Build,
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Page")
            .field("head", &self.head)
            .field("strategy", &self.strategy)
            .field("partial", &self.partial)
            .finish_non_exhaustive()
    }
}

impl Page {
    /// A page whose root is component `C` with `props`.
    #[must_use]
    pub fn new<C: Component>(head: Head, props: C::Props) -> Self
    where
        C::Props: Send + Sync,
    {
        Self {
            head,
            lang: None,
            rtl: false,
            strategy: Strategy::Server,
            partial: false,
            theme: Theme::default(),
            budget: Duration::from_secs(5),
            status: 200,
            build: Arc::new(move |services, theme, scheduler| {
                ComponentTree::with_scheduler(C::new(props.clone()), services, theme, scheduler)
            }),
        }
    }

    /// The document's language (`<html lang>`).
    #[must_use]
    pub fn lang(mut self, lang: &str) -> Self {
        self.lang = Some(lang.to_owned());
        self
    }

    /// A right-to-left document (`<html dir="rtl">`): layout mirrors with no
    /// change to the application.
    #[must_use]
    pub const fn rtl(mut self, rtl: bool) -> Self {
        self.rtl = rtl;
        self
    }

    /// How the page renders.
    #[must_use]
    pub const fn strategy(mut self, strategy: Strategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Partial prerendering (`C06-1`): the page's static shell â€” rendered
    /// once with no request, every boundary a dynamic component is in left
    /// as its fallback â€” is cached and sent at once, and the dynamic parts
    /// are streamed for each request. Implies [`Strategy::Streamed`].
    #[must_use]
    pub const fn partial(mut self) -> Self {
        self.partial = true;
        self.strategy = Strategy::Streamed;
        self
    }

    /// Whether the page is partially prerendered.
    #[must_use]
    pub const fn is_partial(&self) -> bool {
        self.partial
    }

    /// The page's strategy.
    #[must_use]
    pub const fn render_strategy(&self) -> Strategy {
        self.strategy
    }

    /// The theme its styles resolve against.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// How long a render may wait for its data (default 5 seconds): past
    /// it, whatever has not arrived is sent as its fallback.
    #[must_use]
    pub const fn budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// The response status (default 200; a not-found page is 404).
    #[must_use]
    pub const fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// The response status.
    #[must_use]
    pub const fn response_status(&self) -> u16 {
        self.status
    }

    /// The page's head.
    #[must_use]
    pub const fn head(&self) -> &Head {
        &self.head
    }
}

/// Where a page renders: the request (none for a static export or a
/// prerendered shell), the application's services, and where the
/// framework's assets are served.
#[derive(Clone)]
pub struct PageContext {
    /// The request, if the render answers one.
    pub request: Option<RequestInfo>,
    /// The application's services (HTTP, storage, â€¦); the render adds its
    /// own extensions to them.
    pub services: Services,
    /// The URL path the runtime and client modules are served under
    /// (`/_rn/`).
    pub assets: String,
    /// The application's path prefix, for server calls (`""`).
    pub base: String,
    /// The clock the render's tasks measure time with.
    pub clock: Arc<dyn Clock>,
    /// The build's version, which the runtime sends with server calls.
    pub version: Option<String>,
    /// The application's offline and install settings, when it has them:
    /// every page then links the manifest and registers the service worker.
    pub pwa: Option<Arc<crate::pwa::Pwa>>,
}

impl std::fmt::Debug for PageContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageContext")
            .field("request", &self.request)
            .field("assets", &self.assets)
            .finish_non_exhaustive()
    }
}

impl PageContext {
    /// A context for `request`, with no services and the default asset path.
    #[must_use]
    pub fn new(request: Option<RequestInfo>) -> Self {
        Self {
            request,
            services: Services::default(),
            assets: "/_rn/".into(),
            base: String::new(),
            clock: Arc::new(SystemClock),
            version: None,
            pwa: None,
        }
    }

    /// With the application's `services`.
    #[must_use]
    pub fn services(mut self, services: Services) -> Self {
        self.services = services;
        self
    }

    /// With the clock the render's tasks measure time with (a test's
    /// manual clock).
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    fn nonce(&self) -> &str {
        self.request.as_ref().map_or("", |request| request.nonce.as_str())
    }
}

/// A rendered page.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// The document.
    pub html: String,
    /// The response status.
    pub status: u16,
    /// The client modules its islands run, for the server to serve.
    pub modules: Vec<&'static ClientModule>,
    /// Whether any component read the request (a dynamic page).
    pub dynamic: bool,
    /// Whether it has a WebAssembly subtree (its policy then allows
    /// `'wasm-unsafe-eval'`).
    pub wasm: bool,
}

/// Wakes a request thread blocked on its render's tasks.
#[derive(Default)]
struct Signal {
    woken: Mutex<bool>,
    condvar: Condvar,
}

impl Signal {
    fn wake(&self) {
        *self.woken.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.condvar.notify_all();
    }

    fn wait(&self, timeout: Duration) {
        let woken = self.woken.lock().unwrap_or_else(PoisonError::into_inner);
        let (mut woken, _) = self
            .condvar
            .wait_timeout_while(woken, timeout, |woken| !*woken)
            .unwrap_or_else(PoisonError::into_inner);
        *woken = false;
    }
}

/// One request's render: its tree, its executor, and what the render
/// registered.
struct Session {
    tree: ComponentTree,
    executor: HostExecutor,
    render: Arc<ServerRender>,
    signal: Arc<Signal>,
    clock: Arc<dyn Clock>,
    started: Duration,
    theme: Theme,
    csrf: String,
    strategy: Strategy,
    sheet: StyleSheet,
}

impl Session {
    fn start(build: &Build, theme: Theme, strategy: Strategy, cx: &PageContext) -> Self {
        let clock = Arc::clone(&cx.clock);
        let executor = HostExecutor::new(Arc::clone(&clock));
        let signal = Arc::new(Signal::default());
        let wake = Arc::clone(&signal);
        executor.set_wake(Arc::new(move || wake.wake()));
        let scheduler = Scheduler::with_executor(Arc::new(executor.clone()));
        let wake = Arc::clone(&signal);
        scheduler.set_waker(Arc::new(move || wake.wake()));
        let render = Arc::new(ServerRender::new());
        let mut services = cx.services.clone().with_extension(Arc::clone(&render));
        if let Some(request) = &cx.request {
            services = services.with_extension(Arc::new(request.clone()));
        }
        let mut tree = build(services, theme.clone(), scheduler);
        let _ = tree.render();
        Self {
            tree,
            executor,
            render,
            signal,
            started: clock.now(),
            clock,
            theme,
            csrf: cx.request.as_ref().map(|request| request.csrf.clone()).unwrap_or_default(),
            strategy,
            sheet: StyleSheet::new(),
        }
    }

    /// Runs the request's tasks until none is left or `budget` (from the
    /// start of the render) has passed; `true` if they all finished.
    fn settle(&mut self, budget: Duration) -> bool {
        let deadline = self.started.saturating_add(budget);
        loop {
            self.tree.pump_tasks();
            if self.executor.pending_task_count() == 0 && !self.executor.has_ready_work() {
                return true;
            }
            if self.executor.has_ready_work() {
                continue;
            }
            let now = self.clock.now();
            if now >= deadline {
                return false;
            }
            let until = self.executor.next_deadline().map_or(deadline, |timer| timer.min(deadline));
            self.signal.wait(until.saturating_sub(now).max(Duration::from_millis(1)));
        }
    }

    /// The tree as elements, with its islands' specs.
    fn realize(
        &mut self,
        cx: &PageContext,
    ) -> (Element, Vec<Value>, Vec<&'static ClientModule>, bool) {
        let view = self.tree.unresolved_view();
        let islands = self.render.islands();
        let mut marks = Marks {
            csrf: self.csrf.clone(),
            forms: self.render.forms(),
            links: self.render.links(),
            client_only: self.strategy == Strategy::ClientOnly,
            ..Marks::default()
        };
        for (index, island) in islands.iter().enumerate() {
            marks.islands.insert(island.owner, index);
        }
        let mut realizer = Realizer::with_marks(&mut self.sheet, "", &view, &marks);
        let element = realizer.root(&view);
        let flows: HashMap<usize, Value> = realizer.island_flows().into_iter().collect();
        let mut specs = Vec::new();
        let mut modules: Vec<&'static ClientModule> = Vec::new();
        let mut wasm = false;
        for (index, island) in islands.iter().enumerate() {
            let Some(flow) = flows.get(&index) else { continue };
            let mut spec = Map::new();
            spec.insert("i".into(), json!(index));
            spec.insert("s".into(), island.state.clone());
            spec.insert("flow".into(), flow.clone());
            match &island.kind {
                IslandKind::Client(module) => {
                    spec.insert("kind".into(), json!("client"));
                    spec.insert("name".into(), json!(module.name));
                    if island.persist {
                        spec.insert("persist".into(), json!(true));
                    }
                    spec.insert("m".into(), json!(module.url(&cx.assets)));
                    spec.insert("fns".into(), fns(module));
                    if !modules.iter().any(|known| std::ptr::eq(*known, *module)) {
                        modules.push(module);
                    }
                }
                IslandKind::Wasm { module, component, worker } => {
                    wasm = true;
                    spec.insert("kind".into(), json!("wasm"));
                    spec.insert("m".into(), json!(format!("{}w/{module}.wasm", cx.assets)));
                    spec.insert("component".into(), json!(component));
                    if *worker {
                        spec.insert("worker".into(), json!(crate::runtime::worker_url(&cx.assets)));
                    }
                }
                IslandKind::Live { url, then } => {
                    spec.insert("kind".into(), json!("live"));
                    spec.insert("url".into(), json!(format!("{}{url}", cx.base)));
                    if let Some(module) = then {
                        spec.insert("m".into(), json!(module.url(&cx.assets)));
                        spec.insert("fns".into(), fns(module));
                        if !modules.iter().any(|known| std::ptr::eq(*known, *module)) {
                            modules.push(module);
                        }
                    }
                }
            }
            if self.strategy == Strategy::ClientOnly {
                spec.insert("fresh".into(), json!(true));
            }
            specs.push(Value::Object(spec));
        }
        for module in &modules {
            for token in module.tokens {
                self.sheet.note_token(token);
            }
        }
        (element, specs, modules, wasm)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The response is written: every task the request started ends
        // here, finished or not.
        self.executor.shutdown();
    }
}

fn fns(module: &ClientModule) -> Value {
    Value::Object(
        module.server_fns.iter().map(|(name, path)| ((*name).to_owned(), json!(path))).collect(),
    )
}

/// The document up to and including `<body>`.
/// A page's head and document attributes, kept while its tree renders.
struct Document {
    head: Head,
    lang: Option<String>,
    rtl: bool,
}

fn document_start(document: &Document, cx: &PageContext, css: &str, preload: &[String]) -> String {
    let nonce = cx.nonce();
    let mut out = String::from("<!doctype html><html");
    if let Some(lang) = &document.lang {
        out.push_str(" lang=\"");
        escape_into(&mut out, lang);
        out.push('"');
    }
    if document.rtl {
        out.push_str(" dir=\"rtl\"");
    }
    out.push_str("><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    out.push_str(&document.head.render(nonce));
    if let Some(pwa) = &cx.pwa {
        out.push_str(&pwa.head());
    }
    for url in preload {
        out.push_str("<link rel=\"modulepreload\" href=\"");
        escape_into(&mut out, url);
        out.push_str("\">");
    }
    style(&mut out, nonce, css);
    out.push_str("</head><body>");
    out
}

fn style(out: &mut String, nonce: &str, css: &str) {
    out.push_str("<style nonce=\"");
    escape_into(out, nonce);
    out.push_str("\">");
    // `</style` inside the text would end the element; rules never hold it,
    // but a token's value is application text.
    out.push_str(&css.replace("</", "<\\/"));
    out.push_str("</style>");
}

/// The page data and the runtime script, when there are islands; then the
/// end of the document.
fn document_end(out: &mut String, cx: &PageContext, specs: &[Value]) {
    // An offline-capable application registers its service worker from
    // every page, so it ships the runtime even where nothing is interactive.
    if !specs.is_empty() || cx.pwa.is_some() {
        let data = json!({
            "config": {
                "base": cx.base,
                "assets": cx.assets,
                "version": cx.version,
                "sw": cx.pwa.as_ref().map(|_| crate::pwa::SERVICE_WORKER),
            },
            "islands": specs,
        });
        out.push_str("<script type=\"application/json\" id=\"rn-data\">");
        out.push_str(&json_for_script(&data));
        out.push_str("</script><script type=\"module\" src=\"");
        escape_into(out, &crate::runtime::runtime_url(&cx.assets));
        out.push_str("\" nonce=\"");
        escape_into(out, cx.nonce());
        out.push_str("\"></script>");
    }
    out.push_str("</body></html>");
}

/// Renders `page` whole: the document once the render has settled (or its
/// budget has run out).
#[must_use]
pub fn render(page: Page, cx: &PageContext) -> RenderedPage {
    let Page { head, lang, rtl, build, theme, strategy, budget, status, .. } = page;
    let document = Document { head, lang, rtl };
    let mut session = Session::start(&build, theme, strategy, cx);
    session.settle(budget);
    let (element, specs, modules, wasm) = session.realize(cx);
    let css = session.sheet.css(&session.theme);
    let preload: Vec<String> = modules.iter().map(|module| module.url(&cx.assets)).collect();
    let mut html = document_start(&document, cx, &css, &preload);
    render_into(&mut html, &element);
    document_end(&mut html, cx, &specs);
    RenderedPage { html, status, modules, dynamic: session.render.is_dynamic(), wasm }
}

/// A page's static shell (`C06-1`): the document up to its first hole
/// filling, rendered once with no request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// The document's start and the body with each boundary's fallback.
    pub html: String,
    /// The element ids of the boundaries left for each request to fill.
    pub holes: Vec<String>,
    /// The classes the shell's stylesheet already has.
    classes: BTreeSet<String>,
}

/// The script a streamed document defines once: fills a boundary from its
/// `<template>`.
const FILL: &str = "function rnFill(i){var t=document.getElementById(\"rn-f-\"+i),e=document.getElementById(i);if(t&&e)e.replaceWith(t.content);if(t)t.remove()}";

fn fill_script(out: &mut String, nonce: &str) {
    out.push_str("<script nonce=\"");
    escape_into(out, nonce);
    out.push_str("\">");
    out.push_str(FILL);
    out.push_str("</script>");
}

/// The element with id `id` in `element`'s subtree.
fn find<'a>(element: &'a Element, id: &str) -> Option<&'a Element> {
    if element.get("id") == Some(id) {
        return Some(element);
    }
    element.children.iter().find_map(|child| match child {
        Child::Element(child) => find(child, id),
        Child::Text(_) => None,
    })
}

/// The pending boundaries' element ids, by node.
fn boundary_ids(render: &ServerRender) -> Vec<(NodeId, String)> {
    render.boundaries().into_iter().map(|(id, _)| (id, rustnative_core::wire::key_of(id))).collect()
}

/// Renders `page`'s shell with no request, for a partially prerendered
/// route's cache.
#[must_use]
pub fn prerender_shell(page: &Page, cx: &PageContext) -> Shell {
    let cx = PageContext { request: None, ..cx.clone() };
    let document = Document { head: page.head.clone(), lang: page.lang.clone(), rtl: page.rtl };
    let mut session = Session::start(&page.build, page.theme.clone(), page.strategy, &cx);
    // The shell is what renders without waiting: whatever a component
    // loads is a hole.
    session.tree.pump_tasks();
    let (element, _, _, _) = session.realize(&cx);
    let css = session.sheet.css(&session.theme);
    let mut html = document_start(&document, &cx, &css, &[]);
    render_into(&mut html, &element);
    let holes = boundary_ids(&session.render)
        .into_iter()
        .filter(|(id, _)| session.render.unresolved().contains(id))
        .map(|(_, key)| key)
        .collect();
    let classes = session.sheet.classes().map(str::to_owned).collect();
    Shell { html, holes, classes }
}

/// Renders `page` streamed: `emit` receives the shell at once, then each
/// boundary as its data arrives, then the document's end. With `shell`, a
/// partially prerendered route's cached shell is sent instead of rendering
/// one, and the request's render fills its holes.
pub fn render_streamed(
    page: Page,
    cx: &PageContext,
    shell: Option<&Shell>,
    emit: &mut dyn FnMut(String),
) -> RenderedPage {
    let Page { head, lang, rtl, build, theme, strategy, budget, status, .. } = page;
    let document = Document { head, lang, rtl };
    let nonce = cx.nonce().to_owned();
    let mut session = Session::start(&build, theme, strategy, cx);
    session.tree.pump_tasks();

    // The shell: sent before anything is awaited.
    let (mut first, mut sent, mut open): (String, BTreeSet<String>, Vec<String>);
    if let Some(shell) = shell {
        first = shell.html.clone();
        sent = shell.classes.clone();
        open = shell.holes.clone();
    } else {
        let (element, _, _, _) = session.realize(cx);
        let css = session.sheet.css(&session.theme);
        first = document_start(&document, cx, &css, &[]);
        render_into(&mut first, &element);
        sent = session.sheet.classes().map(str::to_owned).collect();
        let unresolved = session.render.unresolved();
        open = boundary_ids(&session.render)
            .into_iter()
            .filter(|(id, _)| unresolved.contains(id))
            .map(|(_, key)| key)
            .collect();
    }
    if !open.is_empty() {
        fill_script(&mut first, &nonce);
    }
    emit(first);

    // Each boundary, as it resolves.
    while !open.is_empty() {
        let step = session.settle_step(budget);
        let unresolved: Vec<String> =
            session.render.unresolved().into_iter().map(rustnative_core::wire::key_of).collect();
        let ready: Vec<String> =
            open.iter().filter(|id| !unresolved.contains(id)).cloned().collect();
        if !ready.is_empty() {
            let (element, _, _, _) = session.realize(cx);
            let mut chunk = String::new();
            let new_rules: Vec<(String, String)> = session
                .sheet
                .entries()
                .into_iter()
                .filter(|(class, _)| !sent.contains(class))
                .collect();
            if !new_rules.is_empty() {
                let mut css = session.sheet.token_css(&session.theme);
                for (class, rule) in new_rules {
                    css.push_str(&rule);
                    sent.insert(class);
                }
                style(&mut chunk, &nonce, &css);
            }
            for id in &ready {
                if let Some(boundary) = find(&element, id) {
                    chunk.push_str("<template id=\"rn-f-");
                    escape_into(&mut chunk, id);
                    chunk.push_str("\">");
                    render_into(&mut chunk, boundary);
                    chunk.push_str("</template><script nonce=\"");
                    escape_into(&mut chunk, &nonce);
                    chunk.push_str("\">rnFill(");
                    chunk.push_str(&json_for_script(&Value::String(id.clone())));
                    chunk.push_str(")</script>");
                }
            }
            emit(chunk);
            open.retain(|id| !ready.contains(id));
        }
        // Idle with boundaries still open (nothing left that could fill
        // them), or out of budget: what is open stays its fallback.
        if step != Some(false) {
            break;
        }
    }

    let (_, specs, modules, wasm) = session.realize(cx);
    let mut end = String::new();
    document_end(&mut end, cx, &specs);
    emit(end);
    RenderedPage {
        html: String::new(),
        status,
        modules,
        dynamic: session.render.is_dynamic(),
        wasm,
    }
}

impl Session {
    /// Waits for the next change, up to `budget` from the start: `None`
    /// when the budget ran out, `Some(true)` when no task is left.
    fn settle_step(&mut self, budget: Duration) -> Option<bool> {
        let deadline = self.started.saturating_add(budget);
        loop {
            let before = self.render.revision();
            self.tree.pump_tasks();
            let idle = self.executor.pending_task_count() == 0 && !self.executor.has_ready_work();
            if idle {
                return Some(true);
            }
            if self.render.revision() != before {
                return Some(false);
            }
            if self.executor.has_ready_work() {
                continue;
            }
            let now = self.clock.now();
            if now >= deadline {
                return None;
            }
            let until = self.executor.next_deadline().map_or(deadline, |timer| timer.min(deadline));
            self.signal.wait(until.saturating_sub(now).max(Duration::from_millis(1)));
        }
    }
}

/// What a page is made of, for development (`/_rn/explain`): which
/// boundaries its shell leaves open, and which components read the request.
#[must_use]
pub fn explain(page: &Page, cx: &PageContext) -> Value {
    let cx = PageContext { request: None, ..cx.clone() };
    let (strategy, partial) = (page.strategy, page.partial);
    let mut session = Session::start(&page.build, page.theme.clone(), strategy, &cx);
    session.tree.pump_tasks();
    let unresolved = session.render.unresolved();
    let boundaries: Vec<Value> = boundary_ids(&session.render)
        .into_iter()
        .map(|(id, key)| json!({ "boundary": key, "static": !unresolved.contains(&id) }))
        .collect();
    json!({
        "strategy": format!("{strategy:?}"),
        "partial": partial,
        "boundaries": boundaries,
        "dynamic_components": session.render.dynamic_count(),
    })
}
