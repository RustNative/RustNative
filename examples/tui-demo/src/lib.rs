//! A to-do list for the terminal: the example of `PLAN.md` Milestone 38.
//!
//! Every interaction is a portable one — the same component would run on
//! any backend — realized by the terminal backend: Tab and Shift+Tab move
//! focus, Enter and Space activate, the list box takes the arrows, typing
//! goes into the field, Ctrl+S is a command, and Ctrl+C closes.

use rustnative_core::{
    Alignment, ColumnStyle, Command, CommandId, Component, ComponentContext, Control, EdgeInsets,
    Event, KeyCode, LayoutStyle, Node, NodeId, RowStyle, Shortcut, SizeMode,
};

/// The command that adds the typed task.
pub const ADD: CommandId = CommandId::new("tui-demo.add");

/// One task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// What to do.
    pub title: String,
    /// Whether it is done.
    pub done: bool,
}

/// The to-do list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    /// The tasks.
    pub tasks: Vec<Task>,
    /// The field's text.
    pub draft: String,
    /// The chosen task.
    pub selected: Option<usize>,
    /// Which tasks are shown: 0 all, 1 open, 2 done.
    pub filter: usize,
}

impl Todo {
    fn shown(&self) -> Vec<usize> {
        (0..self.tasks.len())
            .filter(|index| match self.filter {
                1 => !self.tasks[*index].done,
                2 => self.tasks[*index].done,
                _ => true,
            })
            .collect()
    }

    fn add(&mut self) {
        let title = self.draft.trim().to_owned();
        if !title.is_empty() {
            self.tasks.push(Task { title, done: false });
            self.draft.clear();
            self.selected = Some(self.tasks.len() - 1);
        }
    }
}

fn key(target: NodeId) -> String {
    target.local_key().unwrap_or_default()
}

impl Component for Todo {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self {
            tasks: vec![
                Task { title: "Read the terminal backend's guide".into(), done: true },
                Task { title: "Try Tab, Space, and the arrows".into(), done: false },
                Task { title: "Resize the terminal".into(), done: false },
            ],
            draft: String::new(),
            selected: Some(1),
            filter: 0,
        }
    }

    fn props(&self) -> &() {
        &()
    }

    fn set_props(&mut self, (): ()) {}

    fn view(&self) -> Node {
        Node::column("root", [])
    }

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.command(Command::new(ADD, "Add").shortcut(Shortcut::ctrl(KeyCode::Character('s'))));
        let shown = self.shown();
        let items: Vec<String> = shown
            .iter()
            .map(|index| {
                let task = &self.tasks[*index];
                format!("{} {}", if task.done { "✓" } else { "·" }, task.title)
            })
            .collect();
        let selected =
            self.selected.and_then(|chosen| shown.iter().position(|index| *index == chosen));
        let done = self.tasks.iter().filter(|task| task.done).count();
        let current = self.selected.and_then(|index| self.tasks.get(index));
        Node::column_with_layout(
            "root",
            [
                Node::label("title", "To do"),
                Node::label("summary", format!("{done} of {} done", self.tasks.len())),
                Node::tab_bar("filter", ["All", "Open", "Done"], self.filter, LayoutStyle::new()),
                Node::control_with_layout(
                    "tasks",
                    Control::ListBox { items, selected },
                    LayoutStyle::new().height(SizeMode::Fixed(8 * 16)),
                ),
                Node::control(
                    "done",
                    Control::Checkbox {
                        label: current.map_or_else(
                            || "Done".to_owned(),
                            |task| format!("Done: {}", task.title),
                        ),
                        checked: current.is_some_and(|task| task.done),
                    },
                )
                .disabled(current.is_none()),
                Node::row_with_layout(
                    "new",
                    [
                        Node::text_input_with_layout(
                            "draft",
                            self.draft.clone(),
                            LayoutStyle::new().width(SizeMode::Fill),
                        ),
                        Node::button("add", "Add").with_command(ADD),
                        Node::button("remove", "Remove").disabled(current.is_none()),
                    ],
                    LayoutStyle::new(),
                    RowStyle::new()
                        .padding(EdgeInsets::all(0))
                        .gap(8)
                        .align_items(Alignment::Center),
                ),
                Node::label("help", "Tab moves · Space checks · Ctrl+S adds · Ctrl+C quits"),
            ],
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::symmetric(16, 16)).gap(12),
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::TextChanged { target, value } if key(target) == "draft" => self.draft = value,
            Event::Command { id } if id == ADD => self.add(),
            Event::KeyDown { target: Some(target), key: KeyCode::Enter, .. }
                if key(target) == "draft" =>
            {
                self.add();
            }
            Event::SelectionChanged { target, index } if key(target) == "tasks" => {
                let shown = self.shown();
                self.selected = index.and_then(|index| shown.get(index).copied());
            }
            Event::TabSelected { index, .. } => self.filter = index,
            Event::Toggled { target, on } if key(target) == "done" => {
                if let Some(task) = self.selected.and_then(|index| self.tasks.get_mut(index)) {
                    task.done = on;
                }
            }
            Event::Click { target } if key(target) == "remove" => {
                if let Some(index) = self.selected {
                    self.tasks.remove(index);
                    self.selected = if self.tasks.is_empty() {
                        None
                    } else {
                        Some(index.min(self.tasks.len() - 1))
                    };
                }
            }
            _ => {}
        }
    }
}
