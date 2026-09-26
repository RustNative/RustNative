//! The data layer and error boundaries on the native backend (`PLAN.md`
//! Milestone 47): a query fetched on the executor lands in a native control,
//! an optimistic mutation shows at once and rolls back when rejected, and a
//! panic in an event handler is contained by its boundary — the window
//! stays open, a native fallback replaces the subtree, and "Try again"
//! rebuilds it.

use std::time::{Duration, Instant};

use rustnative_core::{
    Application, Component, ComponentContext, Event, Node, NodeId, Size, SupervisionPolicy, Window,
    WindowId,
};
use rustnative_data::{Mutation, MutationError, Query, QueryClient, QueryError, QueryState};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;

use super::harness::NativeHarness;

struct Board {
    client: Option<QueryClient>,
}

impl Component for Board {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { client: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("board", [])
    }
    fn update(&mut self, event: Event) {
        let (Some(client), Event::Click { target }) = (&self.client, event) else { return };
        if target == NodeId::from_key("add") {
            client.mutate(Mutation::new("add", "rejected item").optimistic::<Vec<String>>(
                ["items"],
                |items| {
                    items.push("rejected item".into());
                },
            ));
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let client = context.provide_scoped_with(|| {
            let client = QueryClient::new();
            client.register_mutation("add", |_| async {
                // Slow enough that the optimistic item is seen first.
                std::thread::sleep(Duration::from_millis(100));
                Err(MutationError::Rejected("no".into()))
            });
            client
        });
        self.client = Some(client.clone());
        let items = match client.use_query(
            context,
            Query::new(["items"], || async {
                Ok::<_, QueryError>(vec!["milk".to_owned(), "eggs".to_owned()])
            }),
        ) {
            QueryState::Success(items) | QueryState::Refreshing(items) => items.join(", "),
            other => format!("{other:?}"),
        };
        let widget =
            context.boundary::<Fragile>("widget", (), SupervisionPolicy::Isolate, |failure| {
                Node::column(
                    "fallback",
                    [
                        Node::label("why", failure.message.clone()),
                        Node::button("retry", "Try again"),
                    ],
                )
            });
        Node::column("board", [Node::label("items", items), Node::button("add", "Add"), widget])
    }
}

struct Fragile;

impl Component for Fragile {
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
        Node::button("explode", "Explode")
    }
    fn update(&mut self, event: Event) {
        assert!(!matches!(event, Event::Click { .. }), "the handler exploded");
    }
}

fn text(hwnd: HWND) -> String {
    let mut buffer = [0_u16; 256];
    // SAFETY: a live control; the buffer's length is passed.
    let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), 256) };
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

/// Pumps until `done` holds, the way the message loop would while tasks
/// finish on the executor.
fn pump_until(harness: &mut NativeHarness, mut done: impl FnMut(&NativeHarness) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        harness.pump();
        if done(harness) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn items(harness: &NativeHarness) -> String {
    harness.control(WindowId::PRIMARY, "items").map(text).unwrap_or_default()
}

#[test]
fn native_data_and_contained_failures() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        if !rustnative_core::scheduler::panic_message(info.payload()).contains("exploded") {
            eprintln!("{info}");
        }
    }));
    let mut application =
        Application::new(Board::new(()), Window::new("data", Size::new(400, 300)));
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    assert!(pump_until(&mut harness, |h| items(h) == "milk, eggs"), "fetched: {}", items(&harness));

    harness.click(WindowId::PRIMARY, "add");
    assert!(
        pump_until(&mut harness, |h| items(h) == "milk, eggs, rejected item"),
        "optimistic: {}",
        items(&harness)
    );
    assert!(
        pump_until(&mut harness, |h| items(h) == "milk, eggs"),
        "rolled back: {}",
        items(&harness)
    );

    let explode = harness.expect_control(WindowId::PRIMARY, "explode");
    harness.click(WindowId::PRIMARY, "explode");
    harness.pump();
    assert!(!harness.quit_requested(), "contained: the application keeps running");
    let why = harness.expect_control(WindowId::PRIMARY, "why");
    assert_eq!(text(why), "the handler exploded");
    assert!(harness.control(WindowId::PRIMARY, "explode").is_none_or(|now| now != explode));
    assert_eq!(items(&harness), "milk, eggs", "the rest of the window is untouched");

    harness.click(WindowId::PRIMARY, "retry");
    harness.pump();
    assert!(harness.control(WindowId::PRIMARY, "explode").is_some(), "rebuilt");
    assert!(harness.control(WindowId::PRIMARY, "why").is_none());
    drop(harness);
    std::panic::set_hook(previous);
}
