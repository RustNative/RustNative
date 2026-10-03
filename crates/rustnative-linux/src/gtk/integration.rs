//! The GTK backend against real widgets on a real display server.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use rustnative_core::{
    Application, Component, ComponentContext, Event, Node, NodeId, Size, Window, WindowId,
};

use super::testing::{Harness, on_gtk, pump_until};

/// A counter: a label, a button, and a field.
struct Counter {
    count: u32,
    text: String,
}

impl Component for Counter {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { count: 0, text: String::new() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("count", format!("Count: {}", self.count)),
                Node::button("increment", "Increment"),
                Node::text_input("field", self.text.clone()),
                Node::label("echo", format!("Typed: {}", self.text)),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => self.count += 1,
            Event::TextChanged { target, value } if target == NodeId::from_key("field") => {
                self.text = value;
            }
            _ => {}
        }
    }
}

#[test]
fn each_node_kind_is_realized_as_the_mapped_widget() {
    on_gtk(|| {
        let mut application =
            Application::new(Counter::new(()), Window::new("Counter", Size::new(320, 240)));
        // SAFETY: `application` outlives `harness`, declared after it.
        let harness = unsafe { Harness::attach(&mut application) };
        assert_eq!(harness.expect("root").type_().name(), "RnLayout");
        assert_eq!(harness.expect("count").type_().name(), "GtkLabel");
        assert_eq!(harness.expect("increment").type_().name(), "GtkButton");
        assert_eq!(harness.expect("field").type_().name(), "GtkEntry");
        let label = harness.expect_as::<gtk::Label>("count");
        assert_eq!(label.text(), "Count: 0");
        assert_eq!(
            harness.gtk_window(WindowId::PRIMARY).and_then(|w| w.title()).as_deref(),
            Some("Counter")
        );
    });
}

#[test]
fn a_click_reaches_the_component_and_is_realized() {
    on_gtk(|| {
        let mut application =
            Application::new(Counter::new(()), Window::new("Counter", Size::new(320, 240)));
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        harness.click("increment");
        harness.click("increment");
        assert_eq!(harness.expect_as::<gtk::Label>("count").text(), "Count: 2");
    });
}

#[test]
fn typing_reports_each_change_and_the_render_does_not_echo() {
    on_gtk(|| {
        let mut application =
            Application::new(Counter::new(()), Window::new("Counter", Size::new(320, 240)));
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        let entry = harness.expect_as::<gtk::Entry>("field");
        entry.set_text("hi");
        harness.pump();
        assert_eq!(harness.expect_as::<gtk::Label>("echo").text(), "Typed: hi");
        // The render set the entry's text to what it already was, which
        // must not have moved the caret or reported a change.
        assert_eq!(entry.text(), "hi");
    });
}

#[test]
fn widgets_are_placed_where_the_layout_engine_put_them() {
    on_gtk(|| {
        let mut application =
            Application::new(Counter::new(()), Window::new("Counter", Size::new(320, 240)));
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        let placed = harness.with_registry(|registry| {
            registry.windows[&WindowId::PRIMARY].renderer.placed_rects().clone()
        });
        for key in ["count", "increment", "field"] {
            let widget = harness.expect(key);
            let rect = placed[&NodeId::from_key(key)];
            let parent = widget.parent().expect("placed in a container");
            // The border box in the container's coordinates: what the
            // container allocated (a widget's own `width()` is its content
            // box, inside the theme's padding).
            let bounds = || widget.compute_bounds(&parent).expect("same tree");
            #[allow(clippy::cast_precision_loss, reason = "small pixel coordinates")]
            let expected = (rect.x as f32, rect.y as f32, rect.width as f32, rect.height as f32);
            let actual = || {
                let bounds = bounds();
                (bounds.x(), bounds.y(), bounds.width(), bounds.height())
            };
            pump_until(&format!("{key} allocated at {rect:?}"), Duration::from_secs(2), || {
                actual() == expected
            });
        }
    });
}

/// Opens a second window, modal to the first.
struct Opener {
    dialogs: Rc<RefCell<u32>>,
}

impl Component for Opener {
    type Props = Rc<RefCell<u32>>;
    type Message = ();
    fn new(dialogs: Self::Props) -> Self {
        Self { dialogs }
    }
    fn props(&self) -> &Self::Props {
        &self.dialogs
    }
    fn set_props(&mut self, dialogs: Self::Props) {
        self.dialogs = dialogs;
    }
    fn view(&self) -> Node {
        Node::button("open", "Open")
    }
    fn update(&mut self, _event: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let _ = context;
        self.view()
    }
}

#[test]
fn a_second_window_opens_and_closes_through_the_application() {
    on_gtk(|| {
        let mut application = Application::new(
            Opener::new(Rc::new(RefCell::new(0))),
            Window::new("Main", Size::new(200, 120)),
        );
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        let second = harness.with_registry(|registry| {
            registry.with_application(|application| {
                application.open_window(
                    Counter::new(()),
                    Window::new("Second", Size::new(200, 150)),
                    None,
                )
            })
        });
        harness.with_registry(super::registry::WindowRegistry::sync).expect("sync");
        harness.settle();
        assert_eq!(harness.live_windows().len(), 2);
        assert_eq!(harness.gtk_window(second).and_then(|w| w.title()).as_deref(), Some("Second"));
        harness.with_registry(|registry| {
            registry.with_application(|application| application.close_window(second))
        });
        harness.with_registry(super::registry::WindowRegistry::sync).expect("sync");
        harness.pump();
        assert_eq!(harness.live_windows(), vec![WindowId::PRIMARY]);
    });
}

type Log = Rc<RefCell<Vec<String>>>;

/// Spawns a task on its first render whose output is its message.
struct TaskRunner {
    props: (Log, bool),
    spawned: bool,
}

impl Component for TaskRunner {
    type Props = (Log, bool);
    type Message = String;
    fn new(props: Self::Props) -> Self {
        Self { props, spawned: false }
    }
    fn props(&self) -> &Self::Props {
        &self.props
    }
    fn set_props(&mut self, props: Self::Props) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::label("status", format!("{} messages", self.props.0.borrow().len()))
    }
    fn update(&mut self, _event: Event) {}
    fn message(&mut self, message: String) {
        self.props.0.borrow_mut().push(message);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, String>) -> Node {
        if !self.spawned {
            self.spawned = true;
            if self.props.1 {
                // `Rc` is `!Send`: this future can only run on the GTK thread.
                let marker = Rc::new("local-task-completed".to_owned());
                context.spawn_local(async move { (*marker).clone() });
            } else {
                // Lands after this render, so the wake is what delivers it.
                let delay = context.sleep(Duration::from_millis(20));
                context.spawn(async move {
                    delay.await;
                    "task-completed".to_owned()
                });
            }
        }
        self.view()
    }
}

#[test]
fn a_task_finishing_on_another_thread_wakes_the_loop() {
    on_gtk(|| {
        for (local, expected) in [(false, "task-completed"), (true, "local-task-completed")] {
            let log: Log = Rc::default();
            let mut application = Application::new(
                TaskRunner::new((log.clone(), local)),
                Window::new("Tasks", Size::new(200, 100)),
            );
            // SAFETY: `application` outlives `harness`.
            let harness = unsafe { Harness::attach(&mut application) };
            pump_until("the task's message", Duration::from_secs(5), || !log.borrow().is_empty());
            assert_eq!(*log.borrow(), vec![expected.to_owned()]);
            harness.pump();
            assert_eq!(harness.expect_as::<gtk::Label>("status").text(), "1 messages");
        }
    });
}

/// Panics from `update`, to drive the panic boundary.
struct Exploder;

impl Component for Exploder {
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
        Node::button("boom", "Boom")
    }
    fn update(&mut self, event: Event) {
        if let Event::Click { .. } = event {
            panic!("component panicked on purpose");
        }
    }
}

#[test]
fn a_panicking_component_is_caught_and_ends_the_run_with_its_message() {
    on_gtk(|| {
        let mut application =
            Application::new(Exploder::new(()), Window::new("Panic", Size::new(200, 100)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        harness.click("boom");
        std::panic::set_hook(previous);
        assert!(harness.finished(), "the default policy ends the run");
        let error = harness.take_error().expect("the panic is the run's failure").to_string();
        assert!(error.contains("component panicked on purpose"), "{error}");
    });
}

#[test]
fn labels_measure_their_text_and_wrap_within_a_width() {
    on_gtk(|| {
        use rustnative_core::{IntrinsicMeasurer, NodeKind};
        let prototypes = super::measure::Prototypes::default();
        let styles = RefCell::new(super::rendering::styling::StyleSheet::install());
        let measurer = super::measure::GtkMeasurer::new(&prototypes, &styles);
        let short = measurer.measure(NodeKind::Label, Some("Hi"), None);
        let long = measurer.measure(NodeKind::Label, Some("Hello there, a longer sentence"), None);
        assert!(long.width > short.width, "{short:?} vs {long:?}");
        assert!(short.height > 0);
        let wrapped =
            measurer.measure(NodeKind::Label, Some("Hello there, a longer sentence"), Some(60));
        assert!(wrapped.width <= 60, "{wrapped:?}");
        assert!(wrapped.height > long.height, "wrapping adds lines: {wrapped:?} vs {long:?}");
        let button = measurer.measure(NodeKind::Button, Some("Hi"), None);
        assert!(
            button.width > short.width && button.height > short.height,
            "a button's chrome: {button:?}"
        );
        styles.borrow().uninstall();
    });
}

/// Sixty rows in a scroll container.
struct Scroller;

impl Component for Scroller {
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
        use rustnative_core::{ColumnStyle, LayoutStyle, Overflow, SizeMode};
        let rows = (0..60).map(|index| {
            Node::label_with_layout(
                format!("row-{index}"),
                format!("Row {index}"),
                LayoutStyle::new().height(SizeMode::Fixed(24)),
            )
        });
        Node::column_with_layout(
            "list",
            rows,
            LayoutStyle::new().height(SizeMode::Fill),
            ColumnStyle::new().overflow(Overflow::Scroll),
        )
    }
    fn update(&mut self, _event: Event) {}
}

#[test]
fn a_scroll_container_is_a_scrolled_window_over_its_whole_content() {
    on_gtk(|| {
        let mut application =
            Application::new(Scroller::new(()), Window::new("Scroll", Size::new(200, 200)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let scrolled = harness.expect_as::<gtk::ScrolledWindow>("list");
        let adjustment = scrolled.vadjustment();
        pump_until("the content size to reach GTK", Duration::from_secs(2), || {
            adjustment.upper() >= 60.0 * 24.0
        });
        assert!(
            adjustment.page_size() < adjustment.upper(),
            "the list is taller than its viewport"
        );
        adjustment.set_value(300.0);
        harness.pump();
        assert!((adjustment.value() - 300.0).abs() < f64::EPSILON, "GTK scrolls its own viewport");
    });
}

#[test]
fn a_canvas_node_is_an_rn_canvas_drawing_its_list() {
    struct Sketch;
    impl Component for Sketch {
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
            use rustnative_core::{Color, DrawList, LayoutStyle, Paint, RectF, SizeMode};
            let list = DrawList::new()
                .fill_rect(RectF::new(0.0, 0.0, 30.0, 30.0), Paint::color(Color::rgb(255, 0, 0)));
            Node::column(
                "root",
                [Node::canvas(
                    "sketch",
                    list,
                    LayoutStyle::new().width(SizeMode::Fixed(60)).height(SizeMode::Fixed(60)),
                )],
            )
        }
        fn update(&mut self, _: Event) {}
    }
    on_gtk(|| {
        let mut application =
            Application::new(Sketch::new(()), Window::new("Canvas", Size::new(200, 120)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let canvas = harness.expect("sketch");
        assert_eq!(canvas.type_().name(), "RnCanvas");
        pump_until("the canvas allocation", Duration::from_secs(2), || canvas.width() > 0);
        // What GTK paints for the widget: the list's red square.
        let paintable = gtk::WidgetPaintable::new(Some(&canvas));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, 60.0, 60.0);
        let node = snapshot.to_node().expect("the canvas draws");
        let renderer = canvas.native().and_then(|native| native.renderer()).expect("a renderer");
        let texture = renderer.render_texture(&node, None);
        let mut downloader = gtk::gdk::TextureDownloader::new(&texture);
        downloader.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = downloader.download_bytes();
        let pixel = |x: usize, y: usize| &bytes[y * stride + x * 4..y * stride + x * 4 + 4];
        assert_eq!(pixel(10, 10), [255, 0, 0, 255], "inside the drawn square");
    });
}

#[test]
fn foreign_widgets_and_media_are_adopted_and_laid_out() {
    struct Host;
    impl Component for Host {
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
            use rustnative_core::{HostContent, LayoutStyle};
            Node::column(
                "root",
                [
                    Node::foreign("picker", "test.spinner", LayoutStyle::new()),
                    Node::foreign(
                        "clip",
                        HostContent::Media { source: "/nonexistent.mp4".into() }.kind(),
                        LayoutStyle::new(),
                    ),
                ],
            )
        }
        fn update(&mut self, _: Event) {}
    }
    on_gtk(|| {
        let made: Rc<RefCell<Option<gtk::Widget>>> = Rc::default();
        let kept = Rc::clone(&made);
        crate::register_foreign("test.spinner", Size::new(40, 30), move || {
            let widget: gtk::Widget = gtk::Spinner::new().upcast();
            *kept.borrow_mut() = Some(widget.clone());
            Some(crate::ForeignWidget { widget, ownership: crate::Ownership::Borrowed })
        });
        let mut application =
            Application::new(Host::new(()), Window::new("Foreign", Size::new(600, 500)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let picker = harness.expect("picker");
        assert_eq!(Some(&picker), made.borrow().as_ref(), "the factory's own widget");
        // A column stretches its children across; the preferred height
        // stands.
        pump_until("the picker at its preferred height", Duration::from_secs(2), || {
            picker.height() == 30
        });
        assert_eq!(harness.expect("clip").type_().name(), "GtkVideo", "media is GTK's own player");
    });
}

/// A mapper extends every button and replaces one label's text, and the
/// inspector lists both (`C24`).
#[test]
fn mappers_extend_and_replace_what_the_backend_applies() {
    use crate::{MappedProperty, MapperMode, MapperTarget, register_mapper};
    use rustnative_core::NodeKind;
    on_gtk(|| {
        register_mapper(
            MapperTarget::Kind(NodeKind::Button),
            MappedProperty::Text,
            MapperMode::Extend,
            |context| {
                context.widget.set_tooltip_text(Some("added by a mapper"));
            },
        );
        register_mapper(
            MapperTarget::Key("count".into()),
            MappedProperty::Text,
            MapperMode::Replace,
            |context| {
                if let Some(label) = context.widget.downcast_ref::<gtk::Label>() {
                    label.set_text("drawn my way");
                }
            },
        );
        let mut application =
            Application::new(Counter::new(()), Window::new("Mappers", Size::new(320, 240)));
        // SAFETY: `application` outlives `harness`, declared after it.
        let harness = unsafe { Harness::attach(&mut application) };
        assert_eq!(
            harness.expect("increment").tooltip_text().as_deref(),
            Some("added by a mapper")
        );
        harness.click("increment");
        assert_eq!(
            harness.expect_as::<gtk::Label>("count").text(),
            "drawn my way",
            "replaced, also on update"
        );
        let listed = harness.with_registry(|registry| {
            use rustnative_core::inspect::InspectBackend as _;
            super::inspect::GtkInspect { runtime: &registry.windows[&WindowId::PRIMARY] }
                .mappers()
                .len()
        });
        assert_eq!(listed, 2);
        crate::clear_mappers();
    });
}

/// Two fixed-width labels in a row.
struct TwoInARow;

impl Component for TwoInARow {
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
        use rustnative_core::{LayoutStyle, SizeMode};
        let fixed = LayoutStyle::new().width(SizeMode::Fixed(80));
        Node::row(
            "row",
            [
                Node::label_with_layout("first", "First", fixed),
                Node::label_with_layout("second", "Second", fixed),
            ],
        )
    }
    fn update(&mut self, _event: Event) {}
}

/// A right-to-left locale mirrors the row once — the engine places, GTK
/// draws each widget's inside right-to-left — and switching back at run
/// time restores it on the same widgets.
///
/// Catches: mirroring applied twice (the engine's rectangles flipped *and*
/// GTK mirroring the container), or not at all.
#[test]
fn a_right_to_left_locale_mirrors_the_window() {
    on_gtk(|| {
        let mut application = Application::new(TwoInARow, Window::new("rtl", Size::new(320, 120)));
        // SAFETY: `application` outlives `harness`, declared after it.
        let harness = unsafe { Harness::attach(&mut application) };
        let set_locale = |tag: &'static str| {
            harness.with_registry(|registry| {
                registry.with_application(|application| {
                    application.set_locale(rustnative_core::Locale::new(tag));
                });
                registry.render(WindowId::PRIMARY).expect("re-render after the locale change");
            });
            harness.pump();
            // GTK allocates at its next frame.
            super::testing::pump_for(Duration::from_millis(50));
        };
        // The desktop's locale is in the environment; the application's
        // own choice overrides it.
        set_locale("ar-EG");
        let window = harness.gtk_window(WindowId::PRIMARY).expect("a window");
        let left = |key: &str| harness.expect(key).compute_bounds(&window).expect("placed").x();
        let first = harness.expect("first");
        assert_eq!(first.direction(), gtk::TextDirection::Rtl, "GTK draws it right-to-left");
        assert!(left("first") > left("second"), "in right-to-left the first item is on the right");

        set_locale("en-GB");
        assert_eq!(harness.expect("first"), first, "the same widget");
        assert_eq!(first.direction(), gtk::TextDirection::Ltr);
        assert!(left("first") < left("second"), "left-to-right again");
    });
}
