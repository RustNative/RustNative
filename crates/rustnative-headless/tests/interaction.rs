//! Component, interaction, and golden tests through the headless backend's
//! real input path.

use std::time::Duration;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, Component, ComponentContext, Event, ItemExtent, KeyCode,
    KeyModifiers, LayoutStyle, Node, NodeId, Size, SizeMode, VirtualListStyle, VirtualRange,
    Window,
};
use rustnative_headless::{HeadlessApp, Query, QueryError, assert_golden};

#[derive(Default)]
struct Form {
    name: String,
    submitted: Option<String>,
    tab: usize,
    presses: u32,
}

impl Component for Form {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self::default()
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}

    fn view(&self) -> Node {
        Node::column(
            "form",
            [
                Node::label("caption", "Your name"),
                Node::text_input("name", self.name.clone()).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput)
                        .labelled_by("caption")
                        .focusable(true),
                ),
                Node::button("submit", "Submit").disabled(self.name.is_empty()),
                Node::label(
                    "result",
                    self.submitted.as_ref().map_or_else(
                        || "Not submitted".to_owned(),
                        |name| format!("Hello, {name}"),
                    ),
                ),
                Node::tab_bar("tabs", ["One", "Two"], self.tab, LayoutStyle::new()),
                Node::label("presses", format!("Keys: {}", self.presses)),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::TextChanged { value, .. } => self.name = value,
            Event::Click { target } if target == NodeId::from_key("submit") => {
                self.submitted = Some(self.name.clone());
            }
            Event::TabSelected { index, .. } => self.tab = index,
            Event::KeyDown { key: KeyCode::Character(_), .. } => self.presses += 1,
            _ => {}
        }
    }
}

fn form() -> HeadlessApp {
    HeadlessApp::launch(Window::new("Form", Size::new(400, 400)), Form::new_default)
}

impl Form {
    fn new_default() -> Self {
        Self::new(())
    }
}

#[test]
fn a_disabled_button_cannot_be_clicked_until_its_condition_holds() -> Result<(), QueryError> {
    let mut app = form();
    let submit = Query::role(AccessibilityRole::Button).name("Submit");
    assert!(matches!(app.click(&submit), Err(QueryError::NotInteractable { .. })));

    app.type_text(&Query::label("Your name"), "Ada")?;
    app.click(&submit)?;
    assert!(app.find(&Query::text("Hello, Ada")).is_ok());
    Ok(())
}

#[test]
fn typing_moves_focus_and_reports_each_keystroke_as_a_host_edit_control_does()
-> Result<(), QueryError> {
    let mut app = form();
    app.type_text(&Query::label("Your name"), "Al")?;
    let field = app.find(&Query::label("Your name"))?;
    assert!(field.focused);
    assert_eq!(field.text.as_deref(), Some("Al"));
    Ok(())
}

#[test]
fn tab_traversal_follows_tree_order_and_skips_disabled_controls() -> Result<(), QueryError> {
    let mut app = form();
    app.tab();
    assert!(app.find(&Query::label("Your name"))?.focused, "first focusable control");
    app.tab();
    // Submit is disabled (the name is empty), so Tab skips it.
    assert!(app.find(&Query::role(AccessibilityRole::TabList))?.focused);
    app.press(KeyCode::Tab, KeyModifiers { shift: true, ..KeyModifiers::default() });
    assert!(app.find(&Query::label("Your name"))?.focused);
    Ok(())
}

#[test]
fn enter_activates_a_focused_button() -> Result<(), QueryError> {
    let mut app = form();
    app.type_text(&Query::label("Your name"), "Bo")?;
    app.tab(); // to Submit, now enabled
    assert!(app.find(&Query::role(AccessibilityRole::Button))?.focused);
    app.press(KeyCode::Enter, KeyModifiers::default());
    assert!(app.find(&Query::text("Hello, Bo")).is_ok());
    Ok(())
}

#[test]
fn tabs_are_selected_through_the_tab_bar() -> Result<(), QueryError> {
    let mut app = form();
    app.select_tab(&Query::role(AccessibilityRole::TabList), 1)?;
    let bar = app.find(&Query::role(AccessibilityRole::TabList))?;
    assert_eq!(bar.tabs.as_ref().map(rustnative_core::Tabs::selected), Some(1));
    Ok(())
}

#[test]
fn a_failed_query_lists_what_the_tree_contains() {
    let app = form();
    let error = app.find(&Query::role(AccessibilityRole::Button).name("Cancel")).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("no node matches `role=Button name=\"Cancel\"`"), "{message}");
    assert!(message.contains("Button \"Submit\""), "{message}");
    assert!(message.contains("TextInput \"Your name\""), "{message}");
}

#[test]
fn the_realized_form_matches_its_golden() {
    let app = form();
    assert_golden!("form-initial", app.golden());
}

// ---------------------------------------------------------------------
// Scrolling: a viewport change, and a virtual list's range change.
// ---------------------------------------------------------------------

struct Rows {
    range: VirtualRange,
    renders: std::rc::Rc<std::cell::Cell<u32>>,
}

thread_local! {
    static RENDERS: std::rc::Rc<std::cell::Cell<u32>> = std::rc::Rc::new(std::cell::Cell::new(0));
}

impl Component for Rows {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        // The range a host would compute for the list's first frame.
        let style = VirtualListStyle::new(10_000, ItemExtent::Fixed(32));
        let extents = rustnative_core::ExtentCache::new(style.item_count, style.extent);
        Self {
            range: VirtualRange::compute(0, 330, &extents, style.overscan),
            renders: RENDERS.with(Clone::clone),
        }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        // Inside a column, so the list gets the fixed height it asks for (a
        // window root always fills the window).
        Node::column(
            "root",
            [Node::virtual_list_with_layout(
                "rows",
                VirtualListStyle::new(10_000, ItemExtent::Fixed(32)),
                LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(330)),
                self.range.indices().map(|index| {
                    Node::label(format!("row-{index}"), format!("Row {index}"))
                        .with_item_index(index)
                }),
            )],
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::VisibleRangeChanged { range, .. } = event {
            self.range = range;
        }
    }
    fn render(&mut self, _context: &mut ComponentContext<'_, ()>) -> Node {
        self.renders.set(self.renders.get() + 1);
        self.view()
    }
}

#[test]
fn scrolling_a_virtual_list_renders_only_when_its_range_moves() -> Result<(), QueryError> {
    let mut app = HeadlessApp::launch(Window::new("Rows", Size::new(300, 400)), || Rows::new(()));
    let renders = RENDERS.with(Clone::clone);
    let before = renders.get();

    // A few pixels: the realized window already covers them (overscan).
    app.scroll(&Query::key("rows"), 4)?;
    assert_eq!(
        renders.get(),
        before,
        "scrolling within a range is a viewport change, not a render"
    );

    // Far enough that different items are needed.
    app.scroll(&Query::key("rows"), 32 * 100)?;
    assert!(renders.get() > before, "a moved range asks the component for new items");
    assert!(app.find(&Query::text("Row 101")).is_ok());
    Ok(())
}

// ---------------------------------------------------------------------
// Time: delays run on virtual time only.
// ---------------------------------------------------------------------

#[derive(Default)]
struct Delayed {
    done: bool,
    started: bool,
}

impl Component for Delayed {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self::default()
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("status", if self.done { "done" } else { "waiting" })
    }
    fn update(&mut self, _event: Event) {}
    fn message(&mut self, (): ()) {
        self.done = true;
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if !self.started {
            self.started = true;
            let delay = context.sleep(Duration::from_secs(30));
            context.spawn(delay);
        }
        self.view()
    }
}

#[test]
fn a_thirty_second_delay_takes_no_real_time() {
    let mut app =
        HeadlessApp::launch(Window::new("Delay", Size::new(200, 100)), || Delayed::new(()));
    assert!(app.find(&Query::text("waiting")).is_ok());
    app.advance(Duration::from_secs(29));
    assert!(app.find(&Query::text("waiting")).is_ok());
    app.advance(Duration::from_secs(1));
    assert!(app.find(&Query::text("done")).is_ok());
}

#[test]
fn a_theme_change_updates_existing_objects_and_creates_none() {
    let mut app = form();
    let before = app.realized().stats();
    app.set_theme(rustnative_core::Theme::default().with_typography(rustnative_core::Typography {
        family: "Serif".into(),
        size: 20,
        weight: 700,
    }));
    let after = app.realized().stats();
    assert_eq!(after.created, before.created, "a theme change is a re-resolution, not a rebuild");
    assert_eq!(after.destroyed, before.destroyed);
}
