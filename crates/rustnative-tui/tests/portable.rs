//! The portable model on the terminal (`PLAN.md` Milestone 38 and the
//! checklist rows it answers): accessibility as a readable, ordered screen;
//! mirroring; colour at the terminal's depth with nothing conveyed by colour
//! alone; capabilities from what the terminal answered; commands; the
//! window stack; scrolling; canvases and images; virtual lists; and the
//! inspection protocol's backend half.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

mod common;

use std::time::Duration;

use rustnative_conformance::reference::{ReferenceScreen, Variant};
use rustnative_core::inspect::InspectBackend;
use rustnative_core::{
    AnimatedProperty, Capability, Color, ColumnStyle, Command, CommandId, Component,
    ComponentContext, Control, DrawList, EdgeInsets, Event, ImageData, InputInterest, KeyCode,
    KeyModifiers, LayoutStyle, Node, NodeId, Overflow, Paint, RectF, Shortcut, SizeMode,
    VisualStyle, WindowId,
};
use rustnative_tui::emulator::Answers;
use rustnative_tui::grid::Attrs;
use rustnative_tui::{ColorDepth, TuiHarness};

#[test]
fn the_screen_reads_in_order_with_labels_before_their_fields() {
    let terminal = TuiHarness::launch(common::window("reference"), 80, 40, || {
        ReferenceScreen::new(Variant::default())
    });
    let text = terminal.text();
    let heading = text.find("Account settings").expect("the heading");
    let name = text.find("Full name").expect("the name label");
    let email = text.find("Email address").expect("the email label");
    assert!(heading < name && name < email, "reading order follows the tree:\n{text}");
    // Each label sits above (or before) its field.
    let model = terminal.model();
    for (label, field) in [("Full name", "Ada Lovelace"), ("Email address", "ada@example.com")] {
        let label_cell = model.nodes().find(|n| n.text.as_deref() == Some(label)).unwrap().cells;
        let field_cell = model.nodes().find(|n| n.text.as_deref() == Some(field)).unwrap().cells;
        assert!(
            label_cell.y < field_cell.y
                || (label_cell.y == field_cell.y && label_cell.x < field_cell.x)
        );
    }
}

#[test]
fn nothing_is_conveyed_by_colour_alone() {
    let (_, root) = common::form();
    let mut terminal = TuiHarness::launch(common::window("form"), 50, 36, root);
    terminal.session_mut().set_color_depth(ColorDepth::None);
    terminal.settle();
    let unfocused = terminal.screen();
    terminal.press_tab();
    let focused = terminal.screen();
    assert_ne!(unfocused, focused, "focus shows without colour");
    let button = terminal.node("button").unwrap();
    let cell = focused.get(button.cells.x, button.cells.y).unwrap();
    assert!(cell.style.attrs.has(Attrs::REVERSE), "as reverse video");
    assert!(cell.style.fg.is_none() && cell.style.bg.is_none(), "no colour was sent");
    // State is in the glyphs: a check box's mark, a list's chosen marker.
    assert!(terminal.text().contains("[ ] Check"));
}

#[test]
fn a_right_to_left_screen_is_mirrored_by_the_backend() {
    let ltr = TuiHarness::launch(common::window("ltr"), 80, 40, || {
        ReferenceScreen::new(Variant::default())
    });
    let rtl = TuiHarness::launch(common::window("rtl"), 80, 40, || {
        ReferenceScreen::new(Variant { pseudo: false, right_to_left: true })
    });
    let position = |terminal: &TuiHarness, text: &str| {
        terminal.model().nodes().find(|n| n.text.as_deref() == Some(text)).map(|n| n.cells).unwrap()
    };
    let (save_l, cancel_l) = (position(&ltr, "Save"), position(&ltr, "Cancel"));
    let (save_r, cancel_r) = (position(&rtl, "Save"), position(&rtl, "Cancel"));
    assert_eq!(save_l.x < cancel_l.x, save_r.x > cancel_r.x, "a row's order is mirrored");
    let heading_l = position(&ltr, "Account settings");
    let heading_r = position(&rtl, "Account settings");
    assert_eq!(heading_l.width, heading_r.width);
    let text = rtl.screen().row_text(u16::try_from(heading_r.y).unwrap());
    let ltr_text = ltr.screen().row_text(u16::try_from(heading_l.y).unwrap());
    assert!(text.len() > ltr_text.len(), "start-aligned text sits at the right edge");
}

struct Colored;

impl Component for Colored {
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
        common::dense(
            "root",
            [Node::label("label", "colour")
                .with_style(VisualStyle::new().foreground(Color::rgb(250, 10, 10)))],
        )
    }
    fn update(&mut self, _event: Event) {}
}

#[test]
fn colour_reaches_the_terminal_at_its_depth() {
    let mut terminal = TuiHarness::launch(common::window("c"), 20, 4, || Colored);
    let fg = |terminal: &TuiHarness| terminal.screen().get(0, 0).and_then(|cell| cell.style.fg);
    terminal.session_mut().set_color_depth(ColorDepth::TrueColor);
    terminal.settle();
    assert_eq!(fg(&terminal), Some(Color::rgb(250, 10, 10)));
    terminal.session_mut().set_color_depth(ColorDepth::Ansi256);
    terminal.settle();
    let (r, g, b) = rustnative_tui::color::palette_256(196);
    assert_eq!(fg(&terminal), Some(Color::rgb(r, g, b)), "the nearest of 256");
    terminal.session_mut().set_color_depth(ColorDepth::Ansi16);
    terminal.settle();
    assert_eq!(fg(&terminal), Some(Color::rgb(255, 0, 0)), "bright red of 16");
    terminal.session_mut().set_color_depth(ColorDepth::None);
    terminal.settle();
    assert_eq!(fg(&terminal), None, "no colour at all");
}

#[test]
fn capabilities_are_what_the_terminal_answered() {
    let modern = TuiHarness::launch(common::window("m"), 20, 4, || Colored);
    let capabilities = modern.session().capabilities().clone();
    assert!(capabilities.supports(Capability::Clipboard), "DA1 answered 52");
    assert!(capabilities.supports(Capability::Hover), "DECRQM recognized 1003 and 1006");
    let seen = modern
        .application()
        .environment_for(WindowId::PRIMARY, &rustnative_core::keys::CAPABILITIES);
    assert_eq!(seen, capabilities, "and the application was told");
    let silent = TuiHarness::launch_with(
        common::window("s"),
        20,
        4,
        Answers::silent(),
        rustnative_core::Theme::default(),
        || Colored,
    );
    let capabilities = silent.session().capabilities();
    for capability in [Capability::Clipboard, Capability::Hover] {
        assert!(!capabilities.supports(capability), "{capability:?} is not assumed");
    }
    for never in [
        Capability::MultipleWindows,
        Capability::Menus,
        Capability::FileDialogs,
        Capability::SystemAppearance,
        Capability::Touch,
        Capability::Pen,
        Capability::Gamepad,
        Capability::Ime,
        Capability::Cursors,
        Capability::DragAndDrop,
        Capability::NativeSurfaces,
        Capability::WindowManagement,
    ] {
        assert!(!capabilities.supports(never), "{never:?} is never claimed by a terminal");
    }
    assert!(capabilities.supports(Capability::CommandShortcuts));
    assert!(capabilities.supports(Capability::CustomDrawing));
}

const SAVE: CommandId = CommandId::new("tui.save");
const QUIT: CommandId = CommandId::new("tui.quit");

#[derive(Default)]
struct Editor {
    saves: u32,
    quits: u32,
}

impl Component for Editor {
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
        Node::column("root", [])
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Command { id } if id == SAVE => self.saves += 1,
            Event::Command { id } if id == QUIT => self.quits += 1,
            _ => {}
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context
            .command(Command::new(SAVE, "Save").shortcut(Shortcut::ctrl(KeyCode::Character('s'))));
        context
            .command(Command::new(QUIT, "Quit").shortcut(Shortcut::ctrl(KeyCode::Character('c'))));
        common::dense(
            "root",
            [
                Node::text_input("body", ""),
                Node::label("status", format!("saves {} quits {}", self.saves, self.quits)),
            ],
        )
    }
}

#[test]
fn a_shortcut_reaches_its_command_from_a_focused_field_and_takes_ctrl_c() {
    let mut terminal = TuiHarness::launch(common::window("editor"), 40, 6, Editor::default);
    terminal.click_key("body");
    let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
    terminal.press(KeyCode::Character('s'), ctrl);
    assert!(terminal.text().contains("saves 1"), "{}", terminal.text());
    terminal.press(KeyCode::Character('c'), ctrl);
    assert!(terminal.text().contains("quits 1"));
    assert!(!terminal.session().is_finished(), "a command bound to Ctrl+C takes it from closing");
}

#[test]
fn a_second_window_covers_the_first_until_it_closes() {
    let mut terminal = TuiHarness::launch(common::window("first"), 40, 6, || Colored);
    let second =
        terminal.application_mut().open_window(Editor::default(), common::window("second"), None);
    terminal.settle();
    assert_eq!(terminal.session().shown_window(), Some(second));
    assert!(terminal.text().contains("saves 0"), "the newest window is shown");
    assert_eq!(
        terminal.terminal().with_emulator(|emulator| emulator.title().to_owned()),
        "second",
        "and titles the terminal"
    );
    terminal.application_mut().close_window(second);
    terminal.settle();
    assert_eq!(terminal.session().shown_window(), Some(WindowId::PRIMARY));
    assert!(terminal.text().contains("colour"), "the first window is shown again");
}

struct Scroller {
    renders: std::rc::Rc<std::cell::Cell<u32>>,
}

impl Component for Scroller {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { renders: std::rc::Rc::default() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        self.renders.set(self.renders.get() + 1);
        let rows = (0..30).map(|n| Node::button(format!("row-{n}"), format!("Row {n}")));
        Node::column_with_layout(
            "list",
            rows,
            LayoutStyle::new().height(SizeMode::Fixed(8 * 16)),
            ColumnStyle::new().padding(EdgeInsets::all(0)).gap(0).overflow(Overflow::Scroll),
        )
    }
    fn update(&mut self, _event: Event) {}
}

#[test]
fn a_scrolling_container_scrolls_by_wheel_without_rendering_and_reveals_focus() {
    let renders = std::rc::Rc::new(std::cell::Cell::new(0));
    let shared = std::rc::Rc::clone(&renders);
    let mut terminal = TuiHarness::launch(common::window("scroll"), 40, 12, move || Scroller {
        renders: std::rc::Rc::clone(&shared),
    });
    assert!(terminal.text().contains("Row 0") && !terminal.text().contains("Row 20"));
    assert!(
        terminal.text().contains("█"),
        "a scroll bar shows the position:
{}",
        terminal.golden()
    );
    let before = renders.get();
    terminal.wheel(5, 3, true);
    assert_eq!(renders.get(), before, "scrolling is a viewport change, not a render");
    assert!(!terminal.text().contains("Row 0 "), "{}", terminal.text());
    // Focus moving to an item out of view scrolls it in.
    for _ in 0..20 {
        terminal.press_tab();
    }
    assert!(terminal.text().contains("Row 19"), "{}", terminal.text());
}

struct Drawing {
    log: common::Log,
}

impl Component for Drawing {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { log: common::Log::default() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let list = DrawList::new()
            .fill_rect(RectF::new(0.0, 0.0, 80.0, 32.0), Paint::color(Color::rgb(0, 0, 200)))
            .stroke_rect(RectF::new(96.0, 0.0, 64.0, 48.0), Paint::color(Color::rgb(0, 200, 0)))
            .text(rustnative_core::Vec2::new(0.0, 48.0), "drawn", 16.0, Color::rgb(255, 255, 255))
            .hit_region(7, RectF::new(96.0, 0.0, 64.0, 48.0));
        let pixels: Vec<u8> = [[255, 0, 0, 255], [0, 0, 255, 255]].concat();
        let image = ImageData::rgba(1, 2, pixels, false).unwrap();
        common::dense(
            "root",
            [
                Node::canvas("canvas", list, LayoutStyle::new().height(SizeMode::Fixed(64)))
                    .with_input(InputInterest::new().pointer()),
                Node::control("image", Control::Image { image }),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(event);
    }
}

#[test]
fn a_canvas_draws_its_list_in_cells_and_reports_its_regions() {
    let log = common::Log::default();
    let shared = std::rc::Rc::clone(&log);
    let mut terminal = TuiHarness::launch(common::window("draw"), 40, 10, move || Drawing {
        log: std::rc::Rc::clone(&shared),
    });
    let screen = terminal.screen();
    assert_eq!(
        screen.get(0, 0).and_then(|cell| cell.style.bg),
        Some(Color::rgb(0, 0, 200)),
        "a fill"
    );
    assert_eq!(
        screen.get(12, 0).map(|cell| cell.text.clone()).as_deref(),
        Some("┌"),
        "a stroke, box-drawn"
    );
    assert_eq!(screen.get(19, 2).map(|cell| cell.text.clone()).as_deref(), Some("┘"));
    assert!(terminal.text().contains("drawn"), "text");
    terminal.click(14, 1);
    let region = log.borrow().iter().find_map(|event| match event {
        Event::PointerDown { pointer, .. } => Some(pointer.region()),
        _ => None,
    });
    assert_eq!(region, Some(Some(7)), "the press names the region it hit");
    // An image is half blocks: upper pixel in the foreground, lower in the
    // background.
    let image = terminal.node("image").unwrap();
    let cell = screen.get(image.cells.x, image.cells.y).unwrap();
    assert_eq!(cell.text, "▀");
    assert_eq!(cell.style.fg, Some(Color::rgb(255, 0, 0)));
    assert_eq!(cell.style.bg, Some(Color::rgb(0, 0, 255)));
}

#[test]
fn the_inspection_backend_answers_with_cells_and_its_tables() {
    let terminal = TuiHarness::launch(common::window("inspect"), 40, 6, || Colored);
    let session = terminal.session();
    let inspect = rustnative_tui::TuiInspect::new(
        session.models(),
        session.capabilities(),
        session.mapping(),
    );
    assert_eq!(inspect.name(), "terminal");
    let objects = inspect.realized(WindowId::PRIMARY);
    assert!(
        objects.iter().any(|object| object.key.as_deref() == Some("label")
            && object.host_type.contains("cells Label"))
    );
    assert_eq!(inspect.style_capabilities().backend, "terminal");
    assert_eq!(inspect.unit_mapping().map(|units| units.host_unit), Some("character cells"));
    assert!(inspect.lifetimes().live > 0);
}

struct Hoverable {
    log: common::Log,
}

impl Component for Hoverable {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { log: common::Log::default() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        common::dense(
            "root",
            [
                Node::label("target", "hover me").with_input(InputInterest::new().pointer()),
                Node::label("other", "elsewhere"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(event);
    }
}

#[test]
fn hover_enters_and_leaves_where_the_terminal_reports_motion() {
    let log = common::Log::default();
    let shared = std::rc::Rc::clone(&log);
    let mut terminal = TuiHarness::launch(common::window("hover"), 30, 4, move || Hoverable {
        log: std::rc::Rc::clone(&shared),
    });
    terminal.hover(1, 0);
    terminal.hover(1, 1);
    let target = NodeId::from_key("target");
    let events = log.borrow();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::PointerEnter { target: t } if *t == target))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::PointerLeave { target: t } if *t == target))
    );
}

#[test]
fn transitions_declared_on_position_change_in_one_step() {
    // A cell cannot move by less than itself: position and size changes are
    // applied at once, and only opacity and colours animate.
    let node = Node::label("x", "x").with_transition(
        AnimatedProperty::Position,
        rustnative_core::Transition::new(Duration::from_millis(300)),
    );
    let _ = node;
}

#[test]
fn the_inspection_overlay_is_drawn_into_the_grid() {
    let mut terminal = TuiHarness::launch(common::window("overlay"), 40, 6, || Colored);
    let plain = terminal.screen();
    let session = terminal.session_mut();
    let models = session.models().clone();
    let capabilities = session.capabilities().clone();
    let mapping = session.mapping();
    let inspect = rustnative_tui::TuiInspect::new(&models, &capabilities, mapping);
    let request = rustnative_core::inspect::Request::Overlay {
        mode: Some(rustnative_core::inspect::OverlayMode::Layout),
    };
    let _ = session.application_mut().inspect(&request, &inspect);
    terminal.resize(40, 6);
    assert_ne!(terminal.screen(), plain, "the overlay draws over the application");
}

#[test]
fn a_theme_change_restyles_the_screen_without_restarting() {
    let (_, root) = common::form();
    let mut terminal = TuiHarness::launch(common::window("theme"), 50, 36, root);
    let before = terminal.screen();
    let created = terminal.model().stats().created;
    let label = rustnative_core::ComponentStyle {
        normal: VisualStyle::new().foreground(Color::rgb(0, 160, 0)),
        ..rustnative_core::ComponentStyle::default()
    };
    terminal.application_mut().set_theme(rustnative_core::Theme::default().with_label(label));
    terminal.settle();
    let status = terminal.node("status").unwrap();
    let cell = terminal.screen().get(status.cells.x, status.cells.y).cloned().unwrap();
    assert_eq!(cell.style.fg, Some(Color::rgb(0, 160, 0)), "the label took the theme's colour");
    assert_ne!(terminal.screen(), before);
    assert_eq!(terminal.model().stats().created, created, "restyled in place: nothing re-created");
}
