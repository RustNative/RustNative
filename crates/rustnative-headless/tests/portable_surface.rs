//! Milestone 39's portable-surface obligations on the headless backend:
//! right-to-left mirrored by the backend itself, container-relative size
//! classes, the safe area, and command shortcuts through the real key path.

use rustnative_core::{
    Command, CommandId, Component, ComponentContext, EdgeInsets, Event, KeyCode, KeyModifiers,
    LayoutStyle, Locale, Node, RowStyle, Shortcut, Size, SizeClass, SizeMode, Window, keys,
};
use rustnative_headless::{HeadlessApp, Query};

struct Pair;

impl Component for Pair {
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
        let fixed = LayoutStyle::new().width(SizeMode::Fixed(80));
        Node::row_with_layout(
            "row",
            [
                Node::label_with_layout("first", "First", fixed),
                Node::label_with_layout("second", "Second", fixed),
            ],
            LayoutStyle::new(),
            RowStyle::new().padding(EdgeInsets::logical(0, 0, 0, 10)),
        )
    }
    fn update(&mut self, _event: Event) {}
}

fn x_of(app: &HeadlessApp, text: &str) -> i32 {
    app.find(&Query::text(text)).map_or(i32::MIN, |node| node.window_rect.x)
}

#[test]
fn a_right_to_left_locale_mirrors_rows_and_start_insets() {
    let mut app = HeadlessApp::launch(Window::new("rtl", Size::new(400, 100)), || Pair::new(()));
    assert_eq!(x_of(&app, "First"), 10, "start padding on the left");
    assert!(x_of(&app, "First") < x_of(&app, "Second"));

    app.application_mut().set_locale(Locale::new("he-IL"));
    app.settle();
    assert_eq!(x_of(&app, "First"), 400 - 10 - 80, "start padding now on the right");
    assert!(x_of(&app, "First") > x_of(&app, "Second"));
}

/// Chooses its arrangement from its own container's width.
struct Adaptive;

impl Component for Adaptive {
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
        Node::column("panel", [])
    }
    fn update(&mut self, _event: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let label = match context.container_classes("panel").map(|classes| classes.width) {
            None => "unmeasured",
            Some(SizeClass::Compact) => "compact",
            Some(_) => "roomy",
        };
        Node::column("panel", [Node::label("arrangement", label)])
    }
}

#[test]
fn a_container_decides_by_its_own_size_class() {
    let mut app =
        HeadlessApp::launch(Window::new("adaptive", Size::new(400, 300)), || Adaptive::new(()));
    assert!(app.find(&Query::text("compact")).is_ok(), "{}", app.golden());
    app.resize(Size::new(1000, 300));
    assert!(app.find(&Query::text("roomy")).is_ok(), "{}", app.golden());
    // The window's class followed too.
    assert_eq!(
        app.application()
            .environment_for(rustnative_core::WindowId::PRIMARY, &keys::SIZE_CLASS)
            .width,
        SizeClass::Expanded
    );
}

#[test]
fn content_stays_clear_of_the_safe_area() {
    let mut app = HeadlessApp::launch(Window::new("safe", Size::new(400, 300)), || Pair::new(()));
    app.application_mut().set_environment(&keys::SAFE_AREA, EdgeInsets::logical(24, 0, 0, 16));
    app.settle();
    let row = app.find(&Query::key("row")).expect("the row");
    assert_eq!((row.window_rect.x, row.window_rect.y), (16, 24));
}

const SAVE: CommandId = CommandId::new("test.save");

struct Saver {
    saves: u32,
}

impl Component for Saver {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { saves: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("saves", "")
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Command { id } if id == SAVE) {
            self.saves += 1;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context
            .command(Command::new(SAVE, "Save").shortcut(Shortcut::ctrl(KeyCode::Character('s'))));
        Node::column(
            "root",
            [Node::text_input("body", ""), Node::label("saves", format!("saves {}", self.saves))],
        )
    }
}

#[test]
fn a_shortcut_reaches_its_command_from_a_focused_field() {
    let mut app = HeadlessApp::launch(Window::new("save", Size::new(300, 200)), || Saver::new(()));
    app.type_text(&Query::key("body"), "x").expect("typing");
    app.press(KeyCode::Character('s'), KeyModifiers { ctrl: true, ..KeyModifiers::default() });
    assert!(app.find(&Query::text("saves 1")).is_ok(), "{}", app.golden());
}

/// The headless backend is a model: it answers every style property
/// "realized", and its unit mapping is one logical pixel per unit
/// (`PLAN.md` Milestone 58).
#[test]
fn the_headless_style_table_realizes_everything() {
    use rustnative_core::Platform;
    let platform = rustnative_headless::HeadlessPlatform::new();
    let table = platform.style_capabilities();
    for (property, support) in table.rows() {
        assert_eq!(support, rustnative_core::StyleSupport::Realized, "`{property}`");
    }
    assert_eq!(platform.unit_mapping().map(|mapping| mapping.host_unit), Some("logical pixels"));
}
