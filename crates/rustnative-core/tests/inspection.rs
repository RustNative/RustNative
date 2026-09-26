//! The inspection protocol (`PLAN.md` Milestone 44), answered by the core:
//! the tree, components and their state, editing, layout and style
//! explanations, tracing with render-or-skip reasons, history, recording
//! and replay, the overlay, and the transport.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use std::time::Duration;

use rustnative_core::inspect::{
    ComponentInfo, HistoryEntry, LayoutExplanation, NoBackend, NodeInfo, Origin, OverlayMode,
    Recording, Reply, Request, StyleExplanation, TraceEntry, TraceKind, send_request,
};
use rustnative_core::{
    Application, Color, Component, ComponentContext, Event, LayoutStyle, Node, NodeId, Size,
    SizeMode, VisualStyle, Window, classes,
};
use serde_json::{Value, json};

/// A counter that shows its state to the inspector and makes it editable.
struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = ();
    type Message = u32;
    fn new((): ()) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "counter",
            [Node::label("count", format!("{}", self.count)), Node::button("increment", "More")],
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.count += 1;
        }
    }
    fn message(&mut self, count: u32) {
        self.count = count;
    }
    fn inspect(&self) -> Option<Value> {
        Some(json!({ "count": self.count }))
    }
    fn edit(&self, field: &str, value: &Value) -> Option<u32> {
        (field == "count").then(|| value.as_u64().and_then(|v| u32::try_from(v).ok())).flatten()
    }
}

/// A form with a styled heading, a password field, and a counter child.
struct Screen {
    name: String,
}

impl Component for Screen {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { name: String::new() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("screen", [])
    }
    fn update(&mut self, event: Event) {
        if let Event::TextChanged { target, value } = event {
            if target == NodeId::from_key("name") {
                self.name = value;
            }
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let counter = context.child::<Counter>("counter");
        Node::column(
            "screen",
            [
                Node::label("heading", "Settings")
                    .with_class(classes!("text-lg hover:text-red-500"))
                    .with_style(VisualStyle::new().background(Color::rgb(1, 2, 3))),
                Node::text_input("name", self.name.clone()),
                Node::text_input("password", ""),
                Node::button_with_layout(
                    "save",
                    "Save",
                    LayoutStyle::new().width(SizeMode::Fixed(120)),
                ),
                counter,
            ],
        )
    }
    fn inspect(&self) -> Option<Value> {
        Some(json!({ "name": self.name }))
    }
}

fn screen() -> Application {
    Application::new(Screen::new(()), Window::new("Screen", Size::new(400, 300)))
}

fn ok(reply: Reply) -> Value {
    match reply {
        Reply::Ok(value) => value,
        Reply::Error(error) => panic!("the inspector refused: {error}"),
    }
}

fn ask<T: serde::de::DeserializeOwned>(app: &mut Application, request: &Request) -> T {
    serde_json::from_value(ok(app.inspect(request, &NoBackend))).unwrap()
}

fn counter_path(app: &Application) -> String {
    app.components()
        .inspect_components()
        .into_iter()
        .find(|component| component.type_name.ends_with("Counter"))
        .unwrap()
        .path
}

fn click(app: &mut Application, component: Option<&str>, key: &str) {
    let target = app.components().find_node(component, key).unwrap();
    app.dispatch(Event::Click { target });
}

#[test]
fn the_tree_names_every_node_its_component_and_its_classes() {
    let mut app = screen();
    let tree: NodeInfo = ask(&mut app, &Request::Tree { window: None });
    assert_eq!(tree.key.as_deref(), Some("screen"));
    let heading = &tree.children[0];
    assert_eq!(heading.text.as_deref(), Some("Settings"));
    assert_eq!(heading.classes, ["text-lg hover:text-red-500"]);
    let counter = tree.children.last().unwrap();
    assert_eq!(counter.key.as_deref(), Some("counter"));
    assert!(counter.component.ends_with("Counter") || counter.component.contains("counter"));
    assert_eq!(counter.children[0].text.as_deref(), Some("0"));
}

#[test]
fn state_is_read_and_edited_through_a_message() {
    let mut app = screen();
    let components: Vec<ComponentInfo> = ask(&mut app, &Request::Components { window: None });
    assert_eq!(components.len(), 2);
    let path = counter_path(&app);
    let state: Value = ask(&mut app, &Request::State { path: path.clone(), window: None });
    assert_eq!(state, json!({ "count": 0 }));

    let edited: ComponentInfo = ask(
        &mut app,
        &Request::SetState {
            path: path.clone(),
            field: "count".into(),
            value: json!(41),
            window: None,
        },
    );
    assert_eq!(edited.state, Some(json!({ "count": 41 })));
    let tree: NodeInfo = ask(&mut app, &Request::Tree { window: None });
    assert_eq!(tree.children.last().unwrap().children[0].text.as_deref(), Some("41"));

    // A field the component does not make editable is refused, not guessed.
    let refused = app.inspect(
        &Request::SetState { path, field: "colour".into(), value: json!(1), window: None },
        &NoBackend,
    );
    assert!(matches!(refused, Reply::Error(message) if message.contains("colour")));
}

#[test]
fn layout_is_explained_by_size_mode_and_parent() {
    let mut app = screen();
    let save: LayoutExplanation =
        ask(&mut app, &Request::ExplainLayout { node: "save".into(), window: None });
    assert_eq!(save.rect[2], 120);
    assert!(!save.realized, "no backend: the shared engine computed it");
    assert!(save.reasons[0].contains("`screen`"), "{:?}", save.reasons);
    assert!(save.reasons.iter().any(|reason| reason.contains("Fixed(120): exactly 120")));
    let name: LayoutExplanation =
        ask(&mut app, &Request::ExplainLayout { node: "name".into(), window: None });
    assert!(
        name.reasons.iter().any(|reason| reason.starts_with("width Fill")),
        "{:?}",
        name.reasons
    );

    // By component and key: the counter's own label.
    let path = counter_path(&app);
    let count: LayoutExplanation =
        ask(&mut app, &Request::ExplainLayout { node: format!("{path}::count"), window: None });
    assert!(count.reasons.iter().any(|reason| reason.starts_with("height Auto")));
    assert!(matches!(
        app.inspect(&Request::ExplainLayout { node: "nothing".into(), window: None }, &NoBackend),
        Reply::Error(_)
    ));
}

#[test]
fn style_is_explained_by_precedence_level() {
    let mut app = screen();
    let heading: StyleExplanation =
        ask(&mut app, &Request::ExplainStyle { node: "heading".into(), window: None });
    let source = |property: &str| {
        heading.sources.iter().filter(|source| source.property == property).collect::<Vec<_>>()
    };
    // The font size from a class, with the class named.
    let size = source("font-size");
    assert_eq!(size[0].origin, Origin::Class { class: "text-lg".into() });
    assert!(size[0].applies);
    // A state variant: written under `hover:`, not applying in the normal
    // state's resolution but listed.
    let color = source("color");
    assert!(color.iter().any(|source| {
        source.origin == Origin::Class { class: "hover:text-red-500".into() }
            && source.condition.as_deref() == Some("hover:")
            && source.resolved.is_some()
    }));
    // A typed override, and the kind's default.
    assert_eq!(source("background-color")[0].origin, Origin::TypedOverride);
    assert!(
        color
            .iter()
            .any(|source| source.origin == Origin::ComponentDefault { kind: "Label".into() })
    );
}

#[test]
fn the_trace_gives_every_component_a_render_or_skip_reason() {
    let mut app = screen();
    let _: Value = ask(&mut app, &Request::Hello);
    let path = counter_path(&app);
    click(&mut app, Some(&path), "increment");
    let trace: Vec<TraceEntry> = ask(&mut app, &Request::Trace { since: 0 });
    let TraceKind::Event { handled, pass: Some(pass), .. } = &trace.last().unwrap().kind else {
        panic!("{trace:?}")
    };
    assert!(handled);
    assert_eq!(pass.rendered.len(), 1);
    assert_eq!(pass.rendered[0].component, path);
    assert_eq!(pass.rendered[0].cause, "event");
    assert_eq!(pass.skipped.len(), 1, "the screen did not render: {pass:?}");

    let history: Vec<HistoryEntry> =
        ask(&mut app, &Request::History { component: Some(path.clone()) });
    assert_eq!(history.last().unwrap().states[&path], json!({ "count": 1 }));
}

#[test]
fn a_recording_redacts_replays_and_becomes_a_test() {
    let mut app = screen();
    let _: Value = ask(&mut app, &Request::StartRecording { redact: vec!["password".into()] });
    let name = app.components().find_node(None, "name").unwrap();
    let password = app.components().find_node(None, "password").unwrap();
    app.dispatch(Event::TextChanged { target: name, value: "Ada".into() });
    app.dispatch(Event::TextChanged { target: password, value: "hunter2".into() });
    let path = counter_path(&app);
    click(&mut app, Some(&path), "increment");
    click(&mut app, Some(&path), "increment");
    let recording: Recording = ask(&mut app, &Request::StopRecording);
    let text = serde_json::to_string(&recording).unwrap();
    assert!(!text.contains("hunter2"), "a redacted value is never recorded");
    assert!(text.contains("[redacted]"));
    let relative = path.strip_prefix(app.components().root_path()).unwrap();
    assert_eq!(relative, "/counter", "recordings name components from the root");
    assert_eq!(recording.final_state[relative], json!({ "count": 2 }));

    // Replay into a fresh application — another run of the same program —
    // skipping the redacted input, reaches the same state.
    let mut replayed = screen();
    let mut without_secret = recording.clone();
    without_secret
        .inputs
        .retain(|input| !serde_json::to_string(input).unwrap().contains("[redacted]"));
    let count = without_secret
        .replay(&mut replayed, rustnative_core::WindowId::PRIMARY, |_, _| {})
        .unwrap();
    assert_eq!(count, 3);
    assert_eq!(replayed.components().relative_states(), recording.final_state);
    // The redacted input itself refuses to replay rather than typing
    // `[redacted]` into the field.
    assert!(
        recording.replay(&mut screen(), rustnative_core::WindowId::PRIMARY, |_, _| {}).is_err()
    );

    let generated = recording.to_test("replays_the_session", "my_app::Screen::new(())");
    assert!(generated.contains("#[test]\nfn replays_the_session()"));
    assert!(generated.contains("app.replay(&recording)"));
}

#[test]
fn the_overlay_is_a_draw_list_over_the_laid_out_nodes() {
    let mut app = screen();
    assert!(
        app.overlay_draw_list(
            rustnative_core::WindowId::PRIMARY,
            &std::collections::HashMap::default()
        )
        .is_none()
    );
    let _: Value = ask(&mut app, &Request::Overlay { mode: Some(OverlayMode::Layout) });
    let rects = std::collections::HashMap::from([(
        NodeId::from_key("save"),
        rustnative_core::Rect::new(10, 20, 120, 30),
    )]);
    let list = app.overlay_draw_list(rustnative_core::WindowId::PRIMARY, &rects).unwrap();
    assert_eq!(list.commands().len(), 2, "an outline and a label");
}

#[test]
fn the_transport_answers_with_the_token_and_refuses_without_it() {
    let mut app = screen();
    let endpoint = app.enable_inspection(None).unwrap();
    assert!(endpoint.addr.ip().is_loopback());
    let client = {
        let endpoint = endpoint.clone();
        std::thread::spawn(move || send_request(&endpoint, &Request::Hello).unwrap())
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !client.is_finished() {
        app.poll_inspection(&NoBackend);
        assert!(std::time::Instant::now() < deadline, "no request arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
    let hello = ok(client.join().unwrap());
    assert_eq!(hello["version"], json!(1));
    assert_eq!(hello["backend"], json!("core"));

    let mut wrong = endpoint;
    wrong.token = "0".repeat(32);
    let refused = send_request(&wrong, &Request::Hello).unwrap();
    assert_eq!(refused, Reply::Error("wrong token".into()));
}

#[test]
fn a_style_file_s_tokens_reach_the_running_application_and_quit_is_a_request() {
    let mut app = screen();
    let token = |app: &Application| {
        app.theme()
            .tokens()
            .resolve(&rustnative_core::StyleValue::Token("color-accent".to_owned().into()))
    };
    assert_eq!(token(&app), None);
    let css = "@theme {\n  --color-accent: #102030;\n}\n";
    let count: usize = ask(&mut app, &Request::SetStyleFile { css: css.into() });
    assert!(count > 0);
    assert_eq!(
        token(&app),
        Some(rustnative_core::StyleValue::Color(Color::rgb(0x10, 0x20, 0x30))),
        "the new token is resolvable without a rebuild"
    );
    assert!(matches!(
        app.inspect(&Request::SetStyleFile { css: "@theme {".into() }, &NoBackend),
        Reply::Error(_)
    ));

    assert!(!app.take_quit_request());
    let _: bool = ask(&mut app, &Request::Quit);
    assert!(app.take_quit_request(), "the backend closes the application");
    assert!(!app.take_quit_request(), "once");
}
