//! The client subset, both ways (`PLAN.md` Web milestone A): each client
//! module below runs natively (its Rust, as rewritten by `#[client]`) and in
//! Node (its generated JavaScript, with the browser runtime), through the
//! same events. After every step the two must hold the same state, realize
//! the same elements, request the same effects, and fail — an overflow, an
//! out-of-bounds index, a division by zero — at the same step with the same
//! message.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::missing_const_for_fn,
    clippy::trivially_copy_pass_by_ref,
    clippy::must_use_candidate,
    clippy::uninlined_format_args,
    clippy::redundant_closure_for_method_calls,
    clippy::single_char_pattern,
    clippy::stable_sort_primitive,
    clippy::explicit_iter_loop,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    missing_docs,
    reason = "test modules written the way an application would write client logic"
)]

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use rustnative_core::{Event, KeyCode, KeyModifiers, NodeId, Services};
use rustnative_web::Effects;
use rustnative_web::client::{ClientLogic, event_json};
use rustnative_web::css::StyleSheet;
use rustnative_web::dom::Realizer;
use serde_json::{Value, json};

#[rustnative_web::client]
pub mod todo {
    use rustnative_core::{
        AccessibilityInfo, AccessibilityRole, Event, KeyCode, LayoutStyle, Node, NodeId, SizeMode,
        rsx,
    };
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Item {
        pub id: u32,
        pub title: String,
        pub done: bool,
    }

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Filter {
        #[default]
        All,
        Active,
        Done,
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Todo {
        pub items: Vec<Item>,
        pub draft: String,
        pub filter: Filter,
        pub next_id: u32,
        pub notice: Option<String>,
    }

    pub enum Msg {
        Stored,
    }

    fn visible(item: &Item, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Active => !item.done,
            Filter::Done => item.done,
        }
    }

    impl Todo {
        fn add(&mut self, fx: &mut Effects<Msg>) {
            let title = self.draft.trim().to_string();
            if title.is_empty() {
                self.notice = Some("Write something first".to_string());
                return;
            }
            self.items.push(Item { id: self.next_id, title, done: false });
            self.next_id += 1;
            self.draft.clear();
            self.notice = None;
            fx.store("todos", &self.items);
        }

        pub fn update(&mut self, event: Event, fx: &mut Effects<Msg>) {
            match event {
                Event::TextChanged { target, value } if target == NodeId::from_key("draft") => {
                    self.draft = value;
                }
                Event::Click { target } if target == NodeId::from_key("add") => self.add(fx),
                Event::KeyDown { key: KeyCode::Enter, .. } => self.add(fx),
                Event::Toggled { target, on } => {
                    for item in self.items.iter_mut() {
                        if target == NodeId::from_key(&format!("item-{}", item.id)) {
                            item.done = on;
                        }
                    }
                }
                Event::TabSelected { index, .. } => {
                    self.filter = match index {
                        0 => Filter::All,
                        1 => Filter::Active,
                        _ => Filter::Done,
                    };
                }
                Event::Click { target } if target == NodeId::from_key("clear") => {
                    self.items.retain(|item| !item.done);
                }
                _ => {}
            }
        }

        pub fn message(&mut self, message: Msg, _fx: &mut Effects<Msg>) {
            match message {
                Msg::Stored => {}
            }
        }

        pub fn view(&self) -> Node {
            let left = self.items.iter().filter(|item| !item.done).count();
            let selected = match self.filter {
                Filter::All => 0,
                Filter::Active => 1,
                Filter::Done => 2,
            };
            rsx! {
                <Column key="todo" gap=8>
                    <Label key="title" text="Things to do" accessibility={AccessibilityInfo::new(AccessibilityRole::Heading { level: 1 })} />
                    <Row key="compose" gap=4>
                        <TextInput key="draft" value={self.draft.clone()} />
                        <Button key="add" text="Add" disabled={self.draft.trim().is_empty()} />
                    </Row>
                    if let Some(notice) = &self.notice {
                        <Label key="notice" text={notice.clone()} class="text-red-600" />
                    }
                    <TabBar key="filter" labels={vec!["All", "Active", "Done"]} selected={selected} />
                    <Column key="list" class="p-2 md:p-4">
                        for item in self.items.iter().filter(|item| visible(item, self.filter)) {
                            <Checkbox key={format!("item-{}", item.id)} label={item.title.clone()} checked={item.done} />
                        }
                    </Column>
                    <Label key="left" text={format!("{} left", left)} width={SizeMode::Fixed(120)} />
                    <Button key="clear" text="Clear done" hidden={self.items.iter().all(|item| !item.done)} />
                </Column>
            }
        }
    }

    // A builder-syntax helper, so both spellings are translated.
    pub fn footer(count: usize) -> Node {
        Node::label_with_layout(
            "footer",
            format!("{count} items"),
            LayoutStyle::new().height(SizeMode::Fill),
        )
    }
}

#[rustnative_web::client]
pub mod numbers {
    use rustnative_core::{Event, Node};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Numbers {
        pub a: i32,
        pub b: i64,
        pub x: f64,
        pub small: u8,
        pub text: String,
        pub log: Vec<String>,
        pub points: Vec<Point>,
        pub scores: Vec<i32>,
        pub pick: Option<usize>,
    }

    impl Numbers {
        pub fn update(&mut self, event: Event, _fx: &mut Effects<()>) {
            let target = match event {
                Event::Click { target } => target,
                Event::TextChanged { value, .. } => {
                    self.text = value;
                    return;
                }
                _ => return,
            };
            let key = target.local_key().unwrap_or_default();
            match key.as_str() {
                "arith" => {
                    self.a = self.a * 3 + 7;
                    self.b -= 1000;
                    self.x = self.x / 3.0 + 0.1;
                    let half = self.a / 2;
                    let rest = self.a % 3;
                    self.log.push(format!("{} {} {} {}", half, rest, -self.a, self.b));
                }
                "format" => {
                    self.log.push(format!(
                        "[{:>8.2}|{:<4}|{:^7}|{:+}|{:x}|{:08.3}|{:?}|{}]",
                        self.x, self.a, "mid", self.a, self.small, self.x, self.text, self.x
                    ));
                    let precise = 0.125_f64;
                    self.log.push(format!("{:.2} {:.0} {:.1}", precise, 2.5_f64, -0.05_f64));
                }
                "text" => {
                    let trimmed = self.text.trim();
                    self.log.push(format!(
                        "{} {} {} {} {}",
                        trimmed.len(),
                        trimmed.chars().count(),
                        trimmed.to_uppercase(),
                        trimmed.contains("é"),
                        trimmed.replace("a", "4")
                    ));
                    match self.text.trim().parse::<i32>() {
                        Ok(value) => self.a = value,
                        Err(error) => self.log.push(error.to_string()),
                    }
                    let words: Vec<String> =
                        self.text.split_whitespace().map(|word| word.to_string()).collect();
                    self.log.push(words.join("+"));
                }
                "collections" => {
                    self.scores.push(self.a);
                    self.scores.push(self.small as i32 * 2);
                    self.scores.push(-4);
                    self.scores.sort();
                    let total: i32 = self.scores.iter().sum();
                    let best = self.scores.iter().max().copied().unwrap_or(0);
                    let first_negative = self.scores.iter().position(|score| *score < 0);
                    self.pick = first_negative;
                    let doubled: Vec<i32> = self
                        .scores
                        .iter()
                        .map(|score| score * 2)
                        .filter(|score| *score != 0)
                        .collect();
                    let labels: Vec<String> = doubled
                        .iter()
                        .enumerate()
                        .map(|(index, value)| format!("{index}:{value}"))
                        .collect();
                    self.log.push(format!(
                        "{} {} {} {}",
                        total,
                        best,
                        first_negative.map_or(-1, |index| index as i32),
                        labels.join(",")
                    ));
                    self.points.push(Point { x: self.a, y: best });
                    self.points.sort_by_key(|point| point.x);
                    let taken = std::mem::take(&mut self.scores);
                    self.log.push(format!("took {}", taken.len()));
                }
                "flow" => {
                    let mut count = 0;
                    let mut total = 0;
                    while count < 5 {
                        count += 1;
                        if count == 2 {
                            continue;
                        }
                        total += count;
                    }
                    let found = loop {
                        total -= 1;
                        if total % 4 == 0 {
                            break total;
                        }
                    };
                    let label = if matches!(self.pick, Some(index) if index > 0) {
                        "later"
                    } else {
                        "first-or-none"
                    };
                    let grade = match self.a {
                        i32::MIN..=-1 => "negative",
                        0 => "zero",
                        1..=99 => "small",
                        _ => "large",
                    };
                    let (left, right) = (self.a.min(10), self.a.max(10));
                    'outer: for i in 0..3 {
                        for j in 0..3 {
                            if i * j == 2 {
                                self.log.push(format!("pair {i} {j}"));
                                break 'outer;
                            }
                        }
                    }
                    if let Some(point) = self.points.first() {
                        if point.x > 0 {
                            self.log.push(format!("first point {}", point.x));
                        }
                    }
                    self.log.push(format!("{total} {found} {label} {grade} {left} {right}"));
                }
                "cast" => {
                    self.small = (self.x * 100.0) as u8;
                    self.a = self.a as u8 as i32;
                    self.log.push(format!("{} {} {}", self.small, self.a, (self.b as f64) / 2.0));
                }
                "overflow" => {
                    self.small += 200;
                }
                "divide" => {
                    self.a = 10 / (self.a - self.a);
                }
                "index" => {
                    self.a = self.scores[5];
                }
                _ => {}
            }
        }

        pub fn view(&self) -> Node {
            Node::column(
                "numbers",
                self.log
                    .iter()
                    .enumerate()
                    .map(|(index, line)| Node::label(format!("line-{index}"), line.clone())),
            )
        }
    }
}

#[rustnative_web::client]
pub mod effects {
    use rustnative_core::api_schema::ApiSchema;
    use rustnative_core::server_fn::{ServerFn, ServerFnError};
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    pub struct Echo;
    impl ServerFn for Echo {
        const PATH: &'static str = "echo";
        type Input = String;
        type Output = String;
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Effectful {
        pub ticks: u32,
        pub answer: String,
        pub loaded: Option<Vec<i32>>,
    }

    pub enum Reply {
        Tick,
        Answered(Result<String, ServerFnError>),
        Loaded(Option<Vec<i32>>),
    }

    impl Effectful {
        pub fn init(&mut self, fx: &mut Effects<Reply>) {
            fx.load::<Vec<i32>>("numbers", Reply::Loaded);
        }

        pub fn update(&mut self, event: Event, fx: &mut Effects<Reply>) {
            if let Event::Click { target } = event {
                if target == NodeId::from_key("tick") {
                    fx.after(250, Reply::Tick);
                } else if target == NodeId::from_key("ask") {
                    fx.call::<Echo>(format!("tick {}", self.ticks), Reply::Answered);
                } else if target == NodeId::from_key("go") {
                    fx.navigate(format!("/items/{}", self.ticks));
                    fx.focus("tick");
                    fx.copy("copied");
                }
            }
        }

        pub fn message(&mut self, reply: Reply, _fx: &mut Effects<Reply>) {
            match reply {
                Reply::Tick => self.ticks += 1,
                Reply::Answered(Ok(text)) => self.answer = text,
                Reply::Answered(Err(error)) => self.answer = format!("failed: {}", error),
                Reply::Loaded(numbers) => self.loaded = numbers,
            }
        }

        pub fn view(&self) -> Node {
            Node::column(
                "effects",
                [
                    Node::button("tick", format!("Ticks: {}", self.ticks)),
                    Node::button("ask", "Ask"),
                    Node::label("answer", self.answer.clone()),
                    Node::button("go", "Go"),
                ],
            )
        }
    }

    impl ApiSchema for Effectful {
        fn schema() -> serde_json::Value {
            serde_json::json!({})
        }
    }
}

/// Polls a future that is ready at once (an effect run with no services).
fn ready<T>(future: Pin<Box<dyn Future<Output = T> + Send>>) -> T {
    let mut future = future;
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("an effect with no services was not ready"),
    }
}

fn realize<S: ClientLogic>(state: &S) -> Value {
    let view = state.view();
    let mut sheet = StyleSheet::new();
    let element = Realizer::new(&mut sheet, "i0-", &view).root(&view);
    json!({ "element": serde_json::to_value(element).unwrap(), "rules": sheet.entries() })
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .unwrap_or_default()
}

/// Runs `events` natively: after each, the state, the realized view, the
/// effects requested, and a panic's message if one happened.
fn native<S: ClientLogic + std::panic::UnwindSafe>(
    initial: S,
    events: &[Event],
    init: bool,
) -> Vec<Value> {
    let mut state = initial;
    let mut out = Vec::new();
    let step = |state: &mut S, run: &mut dyn FnMut(&mut S, &mut Effects<S::Message>)| -> Value {
        let mut fx = Effects::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(state, &mut fx)));
        let mut kinds = Vec::new();
        let mut messages = Vec::new();
        for effect in fx.take() {
            kinds.push(json!([effect.kind, effect.args.clone()]));
            messages.push(effect);
        }
        let mut record = json!({
            "state": serde_json::to_value(&*state).unwrap(),
            "view": realize(state),
            "effects": kinds,
            "panic": result.err().map(panic_message),
        });
        // Answer what a host with no services answers: a call fails, a load
        // finds nothing, a timer fires.
        let services = Services::default();
        let mut replies = Vec::new();
        for effect in messages {
            if let Some(task) = effect.perform(&services) {
                if let Some(message) = ready(task) {
                    replies.push(message);
                }
            }
        }
        let mut after = Vec::new();
        for reply in replies {
            let mut fx = Effects::new();
            state.message(reply, &mut fx);
            after.push(
                json!({ "state": serde_json::to_value(&*state).unwrap(), "view": realize(state) }),
            );
        }
        record["replies"] = Value::Array(after);
        record
    };
    if init {
        out.push(step(&mut state, &mut |state, fx| state.init(fx)));
    }
    for event in events {
        let event = event.clone();
        out.push(step(&mut state, &mut |state, fx| state.update(event.clone(), fx)));
    }
    out
}

const HARNESS: &str = r#"
import * as rn from "./rn.mjs";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
const input = JSON.parse(readFileSync(process.argv[2], "utf8"));
const loaded = await import(pathToFileURL(input.module).href);
const module = loaded.default(rn, input.fns);
const state = input.state;
const realize = () => { const { element, sheet } = rn.realize(module.view(state), "i0-"); return { element, rules: sheet.entries() }; };
const out = [];
const step = (run) => {
  const effects = [];
  const fx = new Proxy({}, { get: (_, kind) => (...args) => effects.push([kind, args]) });
  let panic = null;
  try { run(fx); } catch (error) { if (error instanceof rn.Panic) panic = error.message; else throw error; }
  const record = { state: JSON.parse(JSON.stringify(state)), view: realize(),
    effects: effects.map(([kind, args]) => [kind, kindArgs(kind, args)]), panic };
  const replies = [];
  for (const [kind, args] of effects) {
    let message;
    if (kind === "call") message = args[2]({ Err: { Transport: "no HTTP service" } });
    else if (kind === "after") message = args[1];
    else if (kind === "load") message = args[1](null);
    else continue;
    module.message(state, message, new Proxy({}, { get: () => () => {} }));
    replies.push({ state: JSON.parse(JSON.stringify(state)), view: realize() });
  }
  record.replies = replies;
  out.push(record);
};
// The arguments the Rust side records (functions and messages are not data).
function kindArgs(kind, args) {
  switch (kind) {
    case "call": return [input.paths[args[0]] ?? args[0], args[1]];
    case "after": return [args[0]];
    case "load": return [args[0]];
    default: return args.filter((arg) => typeof arg !== "function");
  }
}
if (input.init && module.init) step((fx) => module.init(state, fx));
for (const event of input.events) step((fx) => module.update(state, event, fx));
process.stdout.write(JSON.stringify(out));
"#;

fn javascript<S: ClientLogic>(initial: &S, events: &[Event], init: bool) -> Option<Vec<Value>> {
    let node = rustnative_web_testing::find_node()?;
    let module = S::MODULE;
    let directory =
        std::env::temp_dir().join(format!("rn-subset-{}-{}", std::process::id(), module.name));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("rn.mjs"), rustnative_web::runtime::RUNTIME_JS).unwrap();
    let module_path = directory.join(format!("{}.mjs", module.name));
    std::fs::write(&module_path, module.js).unwrap();
    std::fs::write(directory.join("harness.mjs"), HARNESS).unwrap();
    let fns: serde_json::Map<String, Value> =
        module.server_fns.iter().map(|(name, path)| ((*name).to_owned(), json!(path))).collect();
    let paths: serde_json::Map<String, Value> =
        module.server_fns.iter().map(|(_, path)| ((*path).to_owned(), json!(path))).collect();
    let input = json!({
        "module": module_path,
        "fns": fns,
        "paths": paths,
        "state": serde_json::to_value(initial).unwrap(),
        "events": events.iter().map(|event| event_json(event).expect("a browser event")).collect::<Vec<_>>(),
        "init": init,
    });
    let input_path = directory.join("input.json");
    std::fs::write(&input_path, input.to_string()).unwrap();
    let output = std::process::Command::new(node)
        .arg(directory.join("harness.mjs"))
        .arg(&input_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "the generated module failed in Node:\n{}\n--- module ---\n{}",
        String::from_utf8_lossy(&output.stderr),
        module.js
    );
    Some(serde_json::from_slice::<Value>(&output.stdout).unwrap().as_array().unwrap().clone())
}

fn sorted_rules(value: &mut Value) {
    if let Some(rules) = value.pointer_mut("/view/rules").and_then(Value::as_array_mut) {
        rules.sort_by_key(ToString::to_string);
    }
    if let Some(replies) = value.get_mut("replies").and_then(Value::as_array_mut) {
        for reply in replies {
            sorted_rules(reply);
        }
    }
}

fn agree<S: ClientLogic + std::panic::UnwindSafe>(initial: S, events: &[Event], init: bool) {
    let Some(js) = javascript(&initial, events, init) else {
        eprintln!("skipped: no node (set RUSTNATIVE_NODE)");
        return;
    };
    let rust = native(initial, events, init);
    assert_eq!(rust.len(), js.len());
    for (index, (mut rust, mut js)) in rust.into_iter().zip(js).enumerate() {
        sorted_rules(&mut rust);
        sorted_rules(&mut js);
        for field in ["panic", "state", "effects", "view", "replies"] {
            assert!(
                rust[field] == js[field],
                "{} step {index}, {field}:\n  rust: {}\n  js:   {}\n--- module ---\n{}",
                S::MODULE.name,
                rust[field],
                js[field],
                S::MODULE.js
            );
        }
    }
}

fn click(key: &str) -> Event {
    Event::Click { target: NodeId::from_key(key) }
}

fn text(key: &str, value: &str) -> Event {
    Event::TextChanged { target: NodeId::from_key(key), value: value.to_owned() }
}

#[test]
fn a_todo_list_agrees_in_both_languages() {
    let events = vec![
        click("add"),
        text("draft", "  Buy milk  "),
        click("add"),
        text("draft", "Walk the dog"),
        Event::KeyDown {
            target: Some(NodeId::from_key("draft")),
            key: KeyCode::Enter,
            modifiers: KeyModifiers::default(),
        },
        Event::Toggled { target: NodeId::from_key("item-0"), on: true },
        Event::TabSelected { target: NodeId::from_key("filter"), index: 1 },
        Event::TabSelected { target: NodeId::from_key("filter"), index: 2 },
        click("clear"),
        Event::TabSelected { target: NodeId::from_key("filter"), index: 0 },
    ];
    agree(todo::Todo::default(), &events, false);
}

#[test]
fn numbers_text_and_control_flow_agree_including_their_failures() {
    let initial =
        numbers::Numbers { a: 7, b: 9_007_199_254_740_000, x: 1.0, small: 3, ..Default::default() };
    let events = vec![
        click("arith"),
        click("format"),
        text("t", "  héllo wörld  "),
        click("text"),
        text("t", "-42"),
        click("text"),
        click("collections"),
        click("flow"),
        click("cast"),
        click("arith"),
        click("overflow"),
        click("overflow"),
        click("divide"),
        click("index"),
    ];
    agree(initial, &events, false);
}

#[test]
fn effects_are_requested_and_answered_alike() {
    let events = vec![click("tick"), click("ask"), click("go"), click("tick")];
    agree(effects::Effectful::default(), &events, true);
}
