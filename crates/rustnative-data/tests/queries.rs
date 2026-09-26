//! The query layer and mutations, on virtual time (`PLAN.md` Milestone 47).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rustnative_core::{
    Component, ComponentContext, ComponentTree, Event, ManualExecutor, MemoryStateStore, Node,
    Scheduler, Services, StateStore, Theme,
};
use rustnative_data::{
    BatchLoader, ConflictPolicy, Mutation, MutationError, Page, Pages, Query, QueryClient,
    QueryError, QueryKey, QueryState, Revalidate,
};

thread_local! {
    static CLIENT: RefCell<Option<QueryClient>> = const { RefCell::new(None) };
    static SHOW: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    static FAILURES_LEFT: RefCell<u32> = const { RefCell::new(0) };
    static SERVER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    // Thread-local: a `ManualExecutor` runs every task on the test's thread,
    // and tests run in parallel.
    static FETCHES: AtomicU32 = const { AtomicU32::new(0) };
    static BATCHES: AtomicU32 = const { AtomicU32::new(0) };
}

fn client() -> QueryClient {
    CLIENT.with(|client| client.borrow().clone().unwrap())
}

/// The root: provides the client, and shows one `Todos` per entry of SHOW.
struct Root;
impl Component for Root {
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
        Node::column("root", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.provide_scoped(client());
        let shown = SHOW.with(|show| show.borrow().clone());
        let children = shown
            .into_iter()
            .map(|key| context.child_with_props::<Todos, _>(key, key, Todos::new))
            .collect::<Vec<_>>();
        Node::column("root", children)
    }
}

fn todos_query() -> Query<Vec<String>> {
    Query::new(["todos"], || {
        let fail = FAILURES_LEFT.with(|left| {
            let mut left = left.borrow_mut();
            let fail = *left > 0;
            *left = left.saturating_sub(1);
            fail
        });
        let items = SERVER.with(|server| server.borrow().clone());
        async move {
            FETCHES.with(|f| f.fetch_add(1, Ordering::SeqCst));
            if fail { Err(QueryError::new("the server is busy")) } else { Ok(items) }
        }
    })
    .empty_when(Vec::is_empty)
    .retries(3, Duration::from_millis(100))
    .stale_time(Duration::from_secs(10))
    .retain_time(Duration::from_secs(60))
    .revalidate(Revalidate {
        on_mount: true,
        on_focus: true,
        on_reconnect: true,
        interval: None,
    })
}

struct Todos {
    key: &'static str,
}
impl Component for Todos {
    type Props = &'static str;
    type Message = ();
    fn new(key: &'static str) -> Self {
        Self { key }
    }
    fn props(&self) -> &&'static str {
        &self.key
    }
    fn set_props(&mut self, key: &'static str) {
        self.key = key;
    }
    fn view(&self) -> Node {
        Node::label("state", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let client = context.scoped::<QueryClient>().unwrap();
        let text = match client.use_query(context, todos_query()) {
            QueryState::Loading => "loading".to_owned(),
            QueryState::Empty => "empty".to_owned(),
            QueryState::Failure(error) => format!("failed: {error}"),
            QueryState::Success(items) => format!("ok: {}", items.join(",")),
            QueryState::Refreshing(items) => format!("refreshing: {}", items.join(",")),
        };
        Node::label("state", text)
    }
}

struct Harness {
    executor: ManualExecutor,
    tree: ComponentTree,
}

impl Harness {
    fn new(show: &[&'static str], server: &[&str]) -> Self {
        FETCHES.with(|f| f.store(0, Ordering::SeqCst));
        CLIENT.with(|client| *client.borrow_mut() = Some(QueryClient::with_seed(7)));
        SHOW.with(|slot| *slot.borrow_mut() = show.to_vec());
        SERVER.with(|slot| *slot.borrow_mut() = server.iter().map(|s| (*s).to_owned()).collect());
        FAILURES_LEFT.with(|left| *left.borrow_mut() = 0);
        let executor = ManualExecutor::new();
        let tree = ComponentTree::with_scheduler(
            Root,
            Services::default(),
            Theme::default(),
            Scheduler::with_executor(Arc::new(executor.clone())),
        );
        let mut harness = Self { executor, tree };
        harness.settle();
        harness
    }

    /// Runs everything that can run without time passing.
    fn settle(&mut self) {
        for _ in 0..20 {
            self.executor.run_until_stalled();
            self.tree.pump_tasks();
        }
    }

    fn advance(&mut self, by: Duration) {
        self.settle();
        self.executor.advance(by);
        self.settle();
    }

    fn states(&self) -> Vec<String> {
        let mut found = Vec::new();
        self.tree.view().visit(&mut |node, _, _| {
            if let Node::Label(label) = node {
                found.push(label.text().to_owned());
            }
        });
        found
    }

    fn show(&mut self, show: &[&'static str]) {
        SHOW.with(|slot| *slot.borrow_mut() = show.to_vec());
        let _ = self.tree.render();
        self.settle();
    }
}

#[test]
fn two_components_asking_for_one_key_share_one_request() {
    let app = Harness::new(&["a", "b"], &["milk", "eggs"]);
    assert_eq!(app.states(), ["ok: milk,eggs", "ok: milk,eggs"]);
    assert_eq!(FETCHES.with(|f| f.load(Ordering::SeqCst)), 1, "deduplicated");
    assert_eq!(client().fetch_count(), 1);
}

#[test]
fn a_fresh_result_is_reused_and_a_stale_one_is_shown_while_it_refreshes() {
    let mut app = Harness::new(&["a"], &["milk"]);
    app.show(&[]);
    app.show(&["a"]);
    assert_eq!(
        FETCHES.with(|f| f.load(Ordering::SeqCst)),
        1,
        "fresh for ten seconds: not fetched again"
    );

    app.show(&[]);
    app.advance(Duration::from_secs(11));
    SERVER.with(|server| server.borrow_mut().push("eggs".into()));
    SHOW.with(|slot| *slot.borrow_mut() = vec!["a"]);
    let _ = app.tree.render();
    app.executor.run_until_stalled();
    app.tree.pump_tasks();
    assert_eq!(app.states(), ["refreshing: milk"], "stale-while-revalidate");
    app.settle();
    assert_eq!(app.states(), ["ok: milk,eggs"]);
}

#[test]
fn an_unobserved_result_is_collected_after_its_retention_time() {
    let mut app = Harness::new(&["a"], &["milk"]);
    let key = QueryKey::from(["todos"]);
    app.show(&[]);
    assert!(client().contains(&key));
    app.advance(Duration::from_secs(61));
    assert!(!client().contains(&key), "collected");
}

#[test]
fn retryable_failures_are_retried_with_backoff() {
    FAILURES_LEFT.with(|left| *left.borrow_mut() = 2);
    let mut app = Harness::new(&[], &["milk"]);
    FAILURES_LEFT.with(|left| *left.borrow_mut() = 2);
    app.show(&["a"]);
    assert_eq!(app.states(), ["loading"], "still trying");
    // 100 ms (+ jitter up to half again), then 200 ms (+ jitter).
    app.advance(Duration::from_millis(150));
    app.advance(Duration::from_millis(300));
    assert_eq!(app.states(), ["ok: milk"]);
    assert_eq!(FETCHES.with(|f| f.load(Ordering::SeqCst)), 3);
}

#[test]
fn exhausted_retries_show_the_failure_and_an_empty_result_is_empty() {
    let mut app = Harness::new(&[], &[]);
    FAILURES_LEFT.with(|left| *left.borrow_mut() = 10);
    app.show(&["a"]);
    for _ in 0..4 {
        app.advance(Duration::from_secs(2));
    }
    assert_eq!(app.states(), ["failed: the server is busy"]);

    FAILURES_LEFT.with(|left| *left.borrow_mut() = 0);
    client().invalidate("todos");
    app.settle();
    assert_eq!(app.states(), ["empty"]);
}

#[test]
fn invalidation_refetches_observed_queries_under_a_prefix() {
    let mut app = Harness::new(&["a"], &["milk"]);
    SERVER.with(|server| server.borrow_mut().push("eggs".into()));
    client().invalidate("todos");
    app.settle();
    assert_eq!(app.states(), ["ok: milk,eggs"]);
    client().invalidate("unrelated");
    app.settle();
    assert_eq!(FETCHES.with(|f| f.load(Ordering::SeqCst)), 2);
}

#[test]
fn an_equal_result_keeps_its_identity_and_re_renders_nobody() {
    let mut app = Harness::new(&["a"], &["milk"]);
    let before = client().data::<Vec<String>>(&QueryKey::from(["todos"])).unwrap();
    client().invalidate("todos");
    app.executor.run_until_stalled();
    app.tree.pump_tasks(); // shows "refreshing"
    app.settle();
    let after = client().data::<Vec<String>>(&QueryKey::from(["todos"])).unwrap();
    assert!(Rc::ptr_eq(&before, &after), "structural sharing");
}

#[test]
fn optimistic_updates_apply_at_once_and_roll_back_when_rejected() {
    let mut app = Harness::new(&["a"], &["milk"]);
    let client = client();
    client.register_mutation("add", |request| async move {
        let item = request.payload.as_str().unwrap_or_default().to_owned();
        if item == "poison" { Err(MutationError::Rejected("no".into())) } else { Ok(()) }
    });
    client.mutate(
        Mutation::new("add", "poison")
            .optimistic::<Vec<String>>(["todos"], |items| items.push("poison".into())),
    );
    app.tree.pump_tasks();
    assert_eq!(app.states(), ["ok: milk,poison"], "shown before the server answers");
    app.settle();
    assert_eq!(app.states(), ["ok: milk"], "rolled back");
    assert_eq!(client.store().read(|cache| cache.mutations().rejected), 1);
}

#[test]
fn offline_mutations_are_queued_durably_and_sent_in_order_on_reconnect() {
    let store = Arc::new(MemoryStateStore::new());
    let mut app = Harness::new(&["a"], &["milk"]);
    let client = client().with_offline_queue(store.clone(), ConflictPolicy::ServerWins);
    let sent = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let log = Arc::clone(&sent);
    client.register_mutation("add", move |request| {
        let log = Arc::clone(&log);
        async move {
            log.lock().push(request.payload.as_str().unwrap_or_default().to_owned());
            Ok(())
        }
    });
    client.set_online(false);
    for item in ["eggs", "bread"] {
        client.mutate(
            Mutation::new("add", item)
                .optimistic::<Vec<String>>(["todos"], move |items| items.push(item.into()))
                .invalidates("todos"),
        );
    }
    app.settle();
    assert_eq!(app.states(), ["ok: milk,eggs,bread"], "optimistic while offline");
    assert_eq!(client.queued_mutations().len(), 2);
    let saved = store.load(rustnative_data::mutation::OFFLINE_QUEUE_KEY).unwrap().unwrap();
    assert!(String::from_utf8(saved).unwrap().contains("bread"), "kept across restarts");

    SERVER.with(|server| server.borrow_mut().extend(["eggs".into(), "bread".into()]));
    client.set_online(true);
    app.settle();
    assert_eq!(*sent.lock(), ["eggs", "bread"], "in order");
    assert!(client.queued_mutations().is_empty());
    assert_eq!(app.states(), ["ok: milk,eggs,bread"]);
}

#[test]
fn a_conflict_is_resolved_by_the_merge_policy() {
    let store = Arc::new(MemoryStateStore::new());
    let mut app = Harness::new(&["a"], &["milk"]);
    let client = client().with_offline_queue(
        store,
        ConflictPolicy::Merge(|client, server| {
            serde_json::json!(format!("{}+{}", client.as_str().unwrap(), server.as_str().unwrap()))
        }),
    );
    let sent = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let log = Arc::clone(&sent);
    client.register_mutation("rename", move |request| {
        let log = Arc::clone(&log);
        async move {
            if request.force {
                log.lock().push(request.payload.as_str().unwrap().to_owned());
                Ok(())
            } else {
                Err(MutationError::Conflict { server: serde_json::json!("theirs") })
            }
        }
    });
    client.mutate(Mutation::new("rename", "mine"));
    app.settle();
    assert_eq!(*sent.lock(), ["mine+theirs"]);
}

// ---------------------------------------------------------------------
// Pagination and batching
// ---------------------------------------------------------------------

struct Feed;
impl Component for Feed {
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
        Node::label("feed", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let client = client();
        let query = Query::infinite(["feed"], |cursor: Option<u32>| async move {
            let start = cursor.unwrap_or(0);
            Ok::<_, QueryError>(Page {
                items: (start..start + 3).collect::<Vec<u32>>(),
                next: (start < 6).then_some(start + 3),
            })
        });
        let text = match client.use_query(context, query) {
            QueryState::Success(pages) | QueryState::Refreshing(pages) => {
                let pages: &Pages<u32, u32> = &pages;
                format!("{:?} more={}", pages.items, pages.next.is_some())
            }
            other => format!("{other:?}"),
        };
        Node::label("feed", text)
    }
}

#[test]
fn an_infinite_query_loads_page_after_page() {
    CLIENT.with(|client| *client.borrow_mut() = Some(QueryClient::new()));
    let executor = ManualExecutor::new();
    let mut tree = ComponentTree::with_scheduler(
        Feed,
        Services::default(),
        Theme::default(),
        Scheduler::with_executor(Arc::new(executor.clone())),
    );
    let settle = |tree: &mut ComponentTree| {
        for _ in 0..10 {
            executor.run_until_stalled();
            tree.pump_tasks();
        }
    };
    settle(&mut tree);
    let text = |tree: &ComponentTree| {
        let mut found = String::new();
        tree.view().visit(&mut |node, _, _| {
            if let Node::Label(label) = node {
                found = label.text().to_owned();
            }
        });
        found
    };
    assert_eq!(text(&tree), "[0, 1, 2] more=true");
    let key = QueryKey::from(["feed"]);
    client().fetch_next_page(&key);
    settle(&mut tree);
    client().fetch_next_page(&key);
    settle(&mut tree);
    assert_eq!(text(&tree), "[0, 1, 2, 3, 4, 5, 6, 7, 8] more=false");
    client().fetch_next_page(&key);
    settle(&mut tree);
    assert_eq!(client().fetch_count(), 3, "no page after the last");
}

struct Authors;
impl Component for Authors {
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
        Node::column("authors", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let background = context.background();
        let loader = context.provide_scoped_with(|| {
            BatchLoader::new(&background, |ids: Vec<u32>| async move {
                BATCHES.with(|f| f.fetch_add(1, Ordering::SeqCst));
                Ok::<_, QueryError>(
                    ids.into_iter()
                        .map(|id| (id, format!("author {id}")))
                        .collect::<HashMap<_, _>>(),
                )
            })
        });
        let rows = (1..=3)
            .map(|id| {
                context.child_with_props::<Author, _>(
                    format!("row-{id}"),
                    (id, loader.clone()),
                    Author::new,
                )
            })
            .collect::<Vec<_>>();
        Node::column("authors", rows)
    }
}

struct Author {
    props: (u32, BatchLoader<u32, String>),
}
impl Component for Author {
    type Props = (u32, BatchLoader<u32, String>);
    type Message = ();
    fn new(props: Self::Props) -> Self {
        Self { props }
    }
    fn props(&self) -> &Self::Props {
        &self.props
    }
    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::label("name", "")
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let (id, loader) = self.props.clone();
        let query =
            Query::local(vec!["author".to_owned(), id.to_string()], move || loader.load(id));
        let text = client()
            .use_query(context, query)
            .data()
            .map_or_else(String::new, |name| (**name).clone());
        Node::label("name", text)
    }
}

#[test]
fn colocated_requirements_are_batched_into_one_request() {
    CLIENT.with(|client| *client.borrow_mut() = Some(QueryClient::new()));
    BATCHES.with(|f| f.store(0, Ordering::SeqCst));
    let executor = ManualExecutor::new();
    let mut tree = ComponentTree::with_scheduler(
        Authors,
        Services::default(),
        Theme::default(),
        Scheduler::with_executor(Arc::new(executor.clone())),
    );
    for _ in 0..10 {
        executor.run_until_stalled();
        tree.pump_tasks();
    }
    let mut names = Vec::new();
    tree.view().visit(&mut |node, _, _| {
        if let Node::Label(label) = node {
            names.push(label.text().to_owned());
        }
    });
    assert_eq!(names, ["author 1", "author 2", "author 3"]);
    assert_eq!(BATCHES.with(|f| f.load(Ordering::SeqCst)), 1, "one request for three rows");
}
