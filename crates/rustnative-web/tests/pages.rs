//! Pages (`PLAN.md` Web milestone H): selective attachment, island
//! markup and page data, rendering that waits for data deterministically,
//! streamed boundaries, partial prerendering, forms without JavaScript,
//! and the request's tasks ending with the response.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::must_use_candidate,
    missing_docs,
    reason = "tests, with client logic written as an application would"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_web::form::form;
use rustnative_web::page::{self, Page, PageContext, Strategy, render, render_streamed};
use rustnative_web::{Client, Head, RequestInfo, pending, request};
use serde_json::Value;

#[rustnative_web::client]
pub mod counter {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Counter {
        pub count: i32,
    }

    impl Counter {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            if let Event::Click { target } = event {
                if target == NodeId::from_key("add") {
                    self.count += 1;
                }
            }
        }

        pub fn view(&self) -> Node {
            Node::row(
                "counter",
                [Node::label("count", format!("{}", self.count)), Node::button("add", "Add")],
            )
        }
    }
}

fn head() -> Head {
    Head::new("Test", "A page for the tests of the page renderer.")
}

fn cx(path: &str) -> PageContext {
    let mut request = RequestInfo::get(path);
    request.nonce = "n0nce".into();
    request.csrf = "t0ken".into();
    PageContext::new(Some(request))
}

fn page_data(html: &str) -> Value {
    let start = html.find("<script type=\"application/json\" id=\"rn-data\">").expect("page data")
        + "<script type=\"application/json\" id=\"rn-data\">".len();
    let end = start + html[start..].find("</script>").unwrap();
    serde_json::from_str(&html[start..end]).unwrap()
}

/// A static root with a client component inside a column.
struct WithIsland;
impl Component for WithIsland {
    type Props = i32;
    type Message = ();
    fn new(_: i32) -> Self {
        Self
    }
    fn props(&self) -> &i32 {
        &0
    }
    fn set_props(&mut self, _: i32) {}
    fn view(&self) -> Node {
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let island = context.child_with_props::<Client<counter::Counter>, _>(
            "counter",
            counter::Counter { count: 3 },
            Client::new,
        );
        Node::column("page", [Node::label("title", "Counter"), island])
    }
}

struct Static;
impl Component for Static {
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
        Node::column("page", [Node::label("title", "Nothing interactive")])
    }
    fn update(&mut self, _: Event) {}
}

#[test]
fn a_page_with_no_island_ships_no_javascript() {
    let rendered = render(Page::new::<Static>(head(), ()), &cx("/"));
    assert!(!rendered.html.contains("<script"), "{}", rendered.html);
    assert!(rendered.modules.is_empty());
    assert!(rendered.html.contains("<style nonce=\"n0nce\">"));
}

#[test]
fn an_island_is_marked_with_its_local_ids_and_its_page_data() {
    let rendered = render(Page::new::<WithIsland>(head(), 0), &cx("/"));
    let html = &rendered.html;
    // The island's root, marked, with ids the browser's realizer gives.
    assert!(html.contains("<div id=\"i0-counter\""), "{html}");
    assert!(html.contains("data-rn-i=\"0\""), "{html}");
    assert!(html.contains("<span id=\"i0-count\""), "{html}");
    assert!(html.contains("<button type=\"button\" id=\"i0-add\""), "{html}");
    // Outside the island, ids are the page's.
    assert!(html.contains("<span id=\"title\""), "{html}");
    let data = page_data(html);
    let island = &data["islands"][0];
    assert_eq!(island["kind"], "client");
    assert_eq!(island["s"], serde_json::json!({ "count": 3 }));
    assert_eq!(island["flow"]["t"], "column");
    let module = counter::__RUSTNATIVE_CLIENT_MODULE.url("/_rn/");
    assert_eq!(island["m"], module);
    // Exactly its module, preloaded, and the runtime.
    assert_eq!(rendered.modules.len(), 1);
    assert!(html.contains(&format!("<link rel=\"modulepreload\" href=\"{module}\">")));
    assert_eq!(html.matches("<script type=\"module\"").count(), 1);
    assert!(html.contains(&rustnative_web::runtime::runtime_url("/_rn/")));
    assert!(html.contains("nonce=\"n0nce\"></script>"));
}

#[test]
fn a_client_only_page_leaves_the_island_to_the_browser() {
    let page = Page::new::<WithIsland>(head(), 0).strategy(Strategy::ClientOnly);
    let rendered = render(page, &cx("/"));
    assert!(rendered.html.contains("data-rn-fresh"), "{}", rendered.html);
    assert!(!rendered.html.contains("id=\"i0-count\""));
    assert_eq!(page_data(&rendered.html)["islands"][0]["fresh"], true);
}

/// A page whose root loads its data in a task.
struct Loads {
    notes: Option<Vec<String>>,
    delay: Duration,
}
impl Component for Loads {
    type Props = u64;
    type Message = Vec<String>;
    fn new(delay: u64) -> Self {
        Self { notes: None, delay: Duration::from_millis(delay) }
    }
    fn props(&self) -> &u64 {
        &0
    }
    fn set_props(&mut self, _: u64) {}
    fn view(&self) -> Node {
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, notes: Vec<String>) {
        self.notes = Some(notes);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, Vec<String>>) -> Node {
        if self.notes.is_none() && context.task_scope().task_count() == 0 {
            let delay = context.sleep(self.delay);
            context.spawn(async move {
                delay.await;
                vec!["first".to_owned(), "second".to_owned()]
            });
        }
        let list = self.notes.as_ref().map(|notes| {
            Node::column(
                "list",
                notes
                    .iter()
                    .enumerate()
                    .map(|(index, note)| Node::label(format!("n{index}"), note.clone())),
            )
        });
        Node::column(
            "page",
            [
                Node::label("title", "Notes"),
                pending(context, "notes", list, || Node::label("loading", "Loading…")),
            ],
        )
    }
}

#[test]
fn a_render_waits_for_its_data_the_same_way_every_time() {
    let first = render(Page::new::<Loads>(head(), 0), &cx("/")).html;
    assert!(first.contains(">second</span>"), "{first}");
    assert!(!first.contains("Loading"), "{first}");
    for _ in 0..50 {
        assert_eq!(render(Page::new::<Loads>(head(), 0), &cx("/")).html, first);
    }
}

#[test]
fn a_streamed_page_sends_the_fallback_first_and_fills_it_in_place() {
    let mut chunks = Vec::new();
    let page = Page::new::<Loads>(head(), 30).strategy(Strategy::Streamed);
    render_streamed(page, &cx("/"), None, &mut |chunk| chunks.push(chunk));
    assert!(chunks.len() >= 3, "{chunks:#?}");
    let shell = &chunks[0];
    assert!(shell.contains(">Loading…</span>") && shell.contains("aria-busy=\"true\""), "{shell}");
    assert!(shell.contains("function rnFill"), "the fill script comes with the shell");
    assert!(!shell.contains(">second<"));
    let fill = &chunks[1];
    assert!(
        fill.contains("<template id=\"rn-f-notes\">") && fill.contains(">second</span>"),
        "{fill}"
    );
    assert!(fill.contains("<script nonce=\"n0nce\">rnFill(\"notes\")</script>"), "{fill}");
    assert!(chunks.last().unwrap().ends_with("</body></html>"));
}

#[test]
fn a_boundary_past_the_budget_stays_its_fallback() {
    let mut chunks = Vec::new();
    let page = Page::new::<Loads>(head(), 5_000)
        .strategy(Strategy::Streamed)
        .budget(Duration::from_millis(50));
    render_streamed(page, &cx("/"), None, &mut |chunk| chunks.push(chunk));
    let all = chunks.concat();
    assert!(all.contains("Loading…") && !all.contains("second"), "{all}");
    assert!(all.ends_with("</body></html>"));
}

/// A page whose greeting reads the request.
struct Greets {
    name: Option<String>,
}
impl Component for Greets {
    type Props = ();
    type Message = String;
    fn new((): ()) -> Self {
        Self { name: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, name: String) {
        self.name = Some(name);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, String>) -> Node {
        if self.name.is_none() && context.task_scope().task_count() == 0 {
            if let Some(request) = request(context) {
                let name = request.query_param("name").unwrap_or_default();
                context.spawn(async move { name });
            }
        }
        let greeting =
            self.name.as_ref().map(|name| Node::label("hello", format!("Hello, {name}")));
        Node::column(
            "page",
            [
                Node::label("title", "Greetings"),
                pending(context, "greeting", greeting, || Node::label("wait", "…")),
            ],
        )
    }
}

#[test]
fn a_partial_page_caches_its_shell_and_streams_its_hole() {
    let page = Page::new::<Greets>(head(), ()).partial();
    let shell = page::prerender_shell(&page, &cx("/greet"));
    assert!(
        shell.html.contains(">Greetings</span>") && shell.html.contains(">…</span>"),
        "{}",
        shell.html
    );
    assert!(!shell.html.contains("Hello"));
    assert_eq!(shell.holes, vec!["greeting".to_owned()]);

    let mut chunks = Vec::new();
    let rendered = render_streamed(page, &cx("/greet?name=Ada"), Some(&shell), &mut |chunk| {
        chunks.push(chunk)
    });
    assert!(chunks[0].starts_with(&shell.html), "the cached shell goes first");
    assert!(chunks.concat().contains(">Hello, Ada</span>"));
    assert!(rendered.dynamic, "the greeting read the request");

    let explained = page::explain(&Page::new::<Greets>(head(), ()).partial(), &cx("/greet"));
    assert_eq!(explained["boundaries"][0]["boundary"], "greeting");
    assert_eq!(explained["boundaries"][0]["static"], false);
}

/// A form of an input, a check box, and a button.
struct NewNote;
impl Component for NewNote {
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
        Node::column("page", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        form(
            context,
            "new",
            "/notes",
            [
                Node::text_input("title", ""),
                Node::checkbox("pinned", "Pinned", false),
                Node::button("save", "Save"),
            ],
        )
    }
}

#[test]
fn a_form_posts_without_javascript() {
    let html = render(Page::new::<NewNote>(head(), ()), &cx("/new")).html;
    assert!(html.contains("<form id=\"new\""), "{html}");
    assert!(html.contains("method=\"post\" action=\"/notes\""), "{html}");
    assert!(html.contains("<input type=\"hidden\" name=\"_csrf\" value=\"t0ken\">"), "{html}");
    assert!(html.contains("<input type=\"text\" name=\"title\""), "{html}");
    assert!(html.contains("name=\"pinned\""), "{html}");
    assert!(html.contains("<button type=\"submit\" name=\"save\" value=\"save\""), "{html}");
}

/// Set when the task holding it is dropped.
#[derive(Clone)]
struct Dropped(Arc<AtomicBool>);
impl PartialEq for Dropped {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// A root whose task would outlive the response.
struct Lingers(Dropped);
impl Component for Lingers {
    type Props = Dropped;
    type Message = ();
    fn new(dropped: Dropped) -> Self {
        Self(dropped)
    }
    fn props(&self) -> &Dropped {
        &self.0
    }
    fn set_props(&mut self, _: Dropped) {}
    fn view(&self) -> Node {
        Node::label("page", "page")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        struct Flag(Arc<AtomicBool>);
        impl Drop for Flag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        if context.task_scope().task_count() == 0 {
            let flag = Flag(Arc::clone(&self.0.0));
            let forever = context.sleep(Duration::from_secs(3600));
            context.spawn(async move {
                forever.await;
                drop(flag);
            });
        }
        self.view()
    }
}

#[test]
fn the_requests_tasks_end_with_the_response() {
    let dropped = Arc::new(AtomicBool::new(false));
    let page = Page::new::<Lingers>(head(), Dropped(Arc::clone(&dropped)))
        .budget(Duration::from_millis(20));
    let rendered = render(page, &cx("/"));
    assert!(rendered.html.contains(">page</span>"));
    assert!(
        dropped.load(Ordering::SeqCst),
        "the task that would outlive the response was dropped with it"
    );
}

#[test]
fn a_document_carries_its_language_direction_and_alternates() {
    let head = head().alternate("fr", "https://example.com/fr/");
    let html = render(Page::new::<Static>(head, ()).lang("ar").rtl(true), &cx("/")).html;
    assert!(html.starts_with("<!doctype html><html lang=\"ar\" dir=\"rtl\"><head>"), "{html}");
    assert!(
        html.contains("<link rel=\"alternate\" hreflang=\"fr\" href=\"https://example.com/fr/\">")
    );
}
