//! Milestone 54's reference application: filtering 200 000 rows as the
//! person types.
//!
//! - **Typing never waits.** The text field's change renders the field and
//!   the status line at once. The filter itself is a deferred value
//!   (`ComponentContext::deferred`): computed off the UI thread, superseded
//!   by the next keystroke, and shown when ready — until then the previous
//!   matches stay on screen, marked as updating.
//! - **Only a screenful exists.** The matches are a virtual list: whatever
//!   their number, only the visible rows are realized.
//! - **A hidden screen does no periodic work.** The filter screen has a
//!   clock that ticks every second on a task spawned with
//!   `SuspendRule::Defer`. Opening the details screen hides the filter
//!   screen, which suspends its tasks: the clock stops until the screen is
//!   shown again, with its state intact.

use std::sync::Arc;
use std::time::Duration;

use rustnative_core::{
    Callback, Component, ComponentContext, Event, ItemExtent, LayoutStyle, Node, NodeId, SizeMode,
    SuspendRule, TaskScope, VirtualListStyle, VirtualRange,
};

/// How many rows the data set holds.
pub const ROWS: usize = 200_000;

const ADJECTIVES: [&str; 16] = [
    "amber", "brisk", "cobalt", "dusky", "eager", "fern", "gilded", "hazel", "ivory", "jade",
    "keen", "lunar", "misty", "noble", "opal", "polar",
];
const NOUNS: [&str; 16] = [
    "falcon", "harbor", "lantern", "meadow", "orchid", "pebble", "quartz", "raven", "summit",
    "thicket", "umber", "valley", "willow", "yarrow", "zephyr", "beacon",
];

/// The data set: `ROWS` rows, the same every run.
#[must_use]
pub fn dataset() -> Arc<Vec<String>> {
    Arc::new(
        (0..ROWS)
            .map(|n| {
                let adjective = ADJECTIVES[n % 16];
                let noun = NOUNS[(n / 16) % 16];
                format!("#{n:06} {adjective} {noun}")
            })
            .collect(),
    )
}

/// The indices of the rows containing `query`, ignoring case.
#[must_use]
pub fn filter(rows: &[String], query: &str) -> Vec<u32> {
    let query = query.to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, row)| row.contains(&query))
        .filter_map(|(index, _)| u32::try_from(index).ok())
        .collect()
}

fn clicked(event: &Event, key: &str) -> bool {
    matches!(event, Event::Click { target } if *target == NodeId::from_key(key))
}

/// The application: the filter screen, and a details screen over it.
pub struct App {
    details: bool,
    rows: Arc<Vec<String>>,
}

impl Component for App {
    type Props = ();
    type Message = bool;

    fn new((): ()) -> Self {
        Self { details: false, rows: dataset() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("app", [])
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, details: bool) {
        self.details = details;
    }
    fn render(&mut self, context: &mut ComponentContext<'_, bool>) -> Node {
        let navigate = context.callback::<bool>();
        let rows = Arc::clone(&self.rows);
        let filter = context
            .child_with_props::<FilterScreen, _>(
                "filter",
                Screen { rows, navigate: navigate.clone() },
                FilterScreen::new,
            )
            .hidden(self.details);
        let mut children = vec![filter];
        if self.details {
            children.push(context.child_with_props::<Details, _>(
                "details",
                navigate,
                Details::new,
            ));
        }
        Node::column_with_layout(
            "app",
            children,
            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fill),
            rustnative_core::ColumnStyle::new(),
        )
    }
}

/// What a screen gets from the application.
#[derive(Clone)]
pub struct Screen {
    rows: Arc<Vec<String>>,
    navigate: Callback<bool>,
}

impl PartialEq for Screen {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.rows, &other.rows) && self.navigate == other.navigate
    }
}

/// The filter screen's clock.
pub enum Tick {
    /// A second passed.
    Second,
}

/// The screen with the query, the status, the clock, and the matches.
pub struct FilterScreen {
    screen: Screen,
    query: String,
    range: VirtualRange,
    ticks: u32,
    scope: Option<TaskScope>,
}

impl FilterScreen {
    /// Arms the clock's next tick, which waits while the screen is hidden.
    fn arm(&self) {
        if let Some(scope) = &self.scope {
            let scheduler = scope.scheduler().clone();
            scope.spawn_with(SuspendRule::Defer, async move {
                scheduler.sleep(Duration::from_secs(1)).await;
                Tick::Second
            });
        }
    }
}

impl Component for FilterScreen {
    type Props = Screen;
    type Message = Tick;

    fn new(screen: Screen) -> Self {
        Self { screen, query: String::new(), range: VirtualRange::EMPTY, ticks: 0, scope: None }
    }
    fn props(&self) -> &Screen {
        &self.screen
    }
    fn set_props(&mut self, screen: Screen) {
        self.screen = screen;
    }
    fn view(&self) -> Node {
        Node::column("filter-screen", [])
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::TextChanged { value, .. } => self.query = value,
            Event::VisibleRangeChanged { range, .. } => self.range = range,
            _ if clicked(&event, "open-details") => self.screen.navigate.send(true),
            _ => {}
        }
    }
    fn message(&mut self, Tick::Second: Tick) {
        self.ticks += 1;
        self.arm();
    }
    fn render(&mut self, context: &mut ComponentContext<'_, Tick>) -> Node {
        if self.scope.is_none() {
            self.scope = Some(context.task_scope());
            self.arm();
        }
        let rows = Arc::clone(&self.screen.rows);
        let matches = context.deferred("matches", self.query.clone(), move |query: String| {
            Arc::new(filter(&rows, &query))
        });
        let shown = matches.current.clone().unwrap_or_default();
        let status = match (&matches.current, matches.pending) {
            (None, _) => "Filtering…".to_owned(),
            (Some(found), pending) => format!(
                "{} of {} rows match{}",
                found.len(),
                self.screen.rows.len(),
                if pending { " — updating…" } else { "" }
            ),
        };
        let items = self.range.indices().filter_map(|index| {
            let row = *shown.get(index)?;
            let text = self.screen.rows.get(usize::try_from(row).ok()?)?.clone();
            Some(
                // Keyed by position, not by row: when the matches change,
                // the visible labels are re-texted rather than destroyed and
                // created again — what keeps a keystroke that lands just
                // after a result cheap.
                Node::label_with_layout(
                    format!("row-{index}"),
                    text,
                    LayoutStyle::new().height(SizeMode::Fixed(22)),
                )
                .with_item_index(index),
            )
        });
        Node::column_with_layout(
            "filter-screen",
            [
                Node::text_input("query", self.query.clone()),
                Node::label("status", status),
                Node::label("clock", format!("Open for {} s", self.ticks)),
                Node::button("open-details", "Details"),
                Node::virtual_list(
                    "matches",
                    VirtualListStyle::new(shown.len(), ItemExtent::Fixed(22)),
                    items,
                ),
            ],
            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fill),
            rustnative_core::ColumnStyle::new(),
        )
    }
}

/// The details screen, shown over the filter screen.
pub struct Details {
    navigate: Callback<bool>,
}

impl Component for Details {
    type Props = Callback<bool>;
    type Message = ();
    fn new(navigate: Callback<bool>) -> Self {
        Self { navigate }
    }
    fn props(&self) -> &Callback<bool> {
        &self.navigate
    }
    fn set_props(&mut self, navigate: Callback<bool>) {
        self.navigate = navigate;
    }
    fn view(&self) -> Node {
        Node::column(
            "details",
            [
                Node::label("about", "The filter screen is hidden, and its clock is stopped."),
                Node::button("back", "Back"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if clicked(&event, "back") {
            self.navigate.send(false);
        }
    }
}
