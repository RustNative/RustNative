//! Components the terminal tests drive.

#![allow(dead_code, reason = "each test file uses part of it")]

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::{
    CalendarDate, ColumnStyle, Component, Control, EdgeInsets, Event, LayoutStyle, Node, NodeId,
    Size, Window,
};

/// Every event a component received, shared with the test.
pub type Log = Rc<RefCell<Vec<Event>>>;

/// A dense column (no padding, no gap), as terminal layouts usually are.
pub fn dense(key: &str, children: impl IntoIterator<Item = Node>) -> Node {
    Node::column_with_layout(
        key,
        children,
        LayoutStyle::new(),
        ColumnStyle::new().padding(EdgeInsets::all(0)).gap(0),
    )
}

/// A window whose size the terminal decides.
pub fn window(title: &str) -> Window {
    Window::new(title, Size::new(0, 0))
}

/// One of everything, answering every event as an application does:
/// the controlled state lives here, and the view renders it.
#[derive(Debug, Clone)]
pub struct Form {
    pub log: Log,
    pub clicks: u32,
    pub text: String,
    pub checked: bool,
    pub radio: usize,
    pub on: bool,
    pub slider: i64,
    pub select: Option<usize>,
    pub list: Option<usize>,
    pub date: CalendarDate,
    pub spin: i64,
    pub tab: usize,
    pub notes: String,
    pub links: u32,
}

impl Form {
    pub fn with_log(log: Log) -> Self {
        Self { log, ..Self::new(()) }
    }
}

impl Component for Form {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self {
            log: Rc::default(),
            clicks: 0,
            text: String::new(),
            checked: false,
            radio: 0,
            on: false,
            slider: 50,
            select: Some(0),
            list: None,
            date: CalendarDate::new(2026, 1, 31).unwrap_or(CalendarDate::new(2026, 1, 1).unwrap()),
            spin: 5,
            tab: 0,
            notes: String::new(),
            links: 0,
        }
    }

    fn props(&self) -> &() {
        &()
    }

    fn set_props(&mut self, (): ()) {}

    fn view(&self) -> Node {
        dense(
            "form",
            [
                Node::label("status", format!("clicks {} links {}", self.clicks, self.links)),
                Node::button("button", "Press"),
                Node::text_input("text", self.text.clone()),
                Node::control(
                    "check",
                    Control::Checkbox { label: "Check".into(), checked: self.checked },
                ),
                Node::control(
                    "radio-a",
                    Control::Radio { label: "A".into(), selected: self.radio == 0 },
                ),
                Node::control(
                    "radio-b",
                    Control::Radio { label: "B".into(), selected: self.radio == 1 },
                ),
                Node::control("toggle", Control::Toggle { label: "Switch".into(), on: self.on }),
                Node::control("slider", Control::Slider { value: self.slider, min: 0, max: 100 }),
                Node::control("progress", Control::Progress { percent: Some(40) }),
                Node::control(
                    "select",
                    Control::Select {
                        options: vec!["Red".into(), "Green".into(), "Blue".into()],
                        selected: self.select,
                    },
                ),
                Node::control(
                    "list",
                    Control::ListBox {
                        items: (1..=6).map(|n| format!("Item {n}")).collect(),
                        selected: self.list,
                    },
                ),
                Node::control("date", Control::DatePicker { date: self.date }),
                Node::control("spin", Control::Spinner { value: self.spin, min: 0, max: 9 }),
                Node::control("rule", Control::Separator),
                Node::control("link", Control::Link { text: "Details".into() }),
                Node::tab_bar("tabs", ["First", "Second", "Third"], self.tab, LayoutStyle::new()),
                Node::control("notes", Control::MultilineText { value: self.notes.clone() }),
                Node::button("off", "Disabled").disabled(true),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(event.clone());
        let key = |target: NodeId| target.local_key().unwrap_or_default();
        match event {
            Event::Click { target } if key(target) == "button" => self.clicks += 1,
            Event::Click { target } if key(target) == "link" => self.links += 1,
            Event::TextChanged { target, value } if key(target) == "text" => self.text = value,
            Event::TextChanged { target, value } if key(target) == "notes" => self.notes = value,
            Event::Toggled { target, on } => match key(target).as_str() {
                "check" => self.checked = on,
                "toggle" => self.on = on,
                "radio-a" if on => self.radio = 0,
                "radio-b" if on => self.radio = 1,
                _ => {}
            },
            Event::ValueChanged { target, value } if key(target) == "slider" => self.slider = value,
            Event::ValueChanged { target, value } if key(target) == "spin" => self.spin = value,
            Event::SelectionChanged { target, index } if key(target) == "select" => {
                self.select = index;
            }
            Event::SelectionChanged { target, index } if key(target) == "list" => self.list = index,
            Event::DateChanged { date, .. } => self.date = date,
            Event::TabSelected { index, .. } => self.tab = index,
            _ => {}
        }
    }
}

/// A form and the log of what it was told.
pub fn form() -> (Log, impl Fn() -> Form) {
    let log: Log = Rc::default();
    let shared = Rc::clone(&log);
    (log, move || Form::with_log(Rc::clone(&shared)))
}

/// The events of `log` that are not focus or key bookkeeping.
pub fn changes(log: &Log) -> Vec<Event> {
    log.borrow()
        .iter()
        .filter(|event| {
            !matches!(
                event,
                Event::FocusGained { .. }
                    | Event::FocusLost { .. }
                    | Event::KeyDown { .. }
                    | Event::KeyUp { .. }
                    | Event::WindowResized { .. }
                    | Event::PointerMove { .. }
            )
        })
        .cloned()
        .collect()
}
