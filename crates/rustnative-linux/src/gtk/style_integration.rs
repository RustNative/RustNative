//! Controls, styling, and host traits on real GTK widgets (Phase 2).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{
    Application, CalendarDate, ColorScheme, Component, Control, Event, ImageData, Node, NodeId,
    Size, Window, WindowId, keys,
};

use super::testing::{Harness, on_gtk};
use crate::desktop::fake_bus::{FakeService, Object};

type Log = Rc<RefCell<Vec<Event>>>;

/// Every control kind, each rendering the state its last event reported.
struct Controls {
    log: Log,
    checked: bool,
    radio: usize,
    on: bool,
    slider: i64,
    select: Option<usize>,
    list: Option<usize>,
    date: CalendarDate,
    spinner: i64,
    text: String,
}

impl Component for Controls {
    type Props = Log;
    type Message = ();
    fn new(log: Log) -> Self {
        Self {
            log,
            checked: false,
            radio: 0,
            on: false,
            slider: 10,
            select: Some(0),
            list: None,
            date: CalendarDate::new(2026, 9, 29).unwrap_or_default(),
            spinner: 3,
            text: "one".to_owned(),
        }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        let image = ImageData::rgba(2, 2, vec![255_u8; 16], false).expect("a 2×2 image");
        Node::column(
            "root",
            [
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
                Node::control("toggle", Control::Toggle { label: "Toggle".into(), on: self.on }),
                Node::control("slider", Control::Slider { value: self.slider, min: 0, max: 100 }),
                Node::control("progress", Control::Progress { percent: Some(40) }),
                Node::control(
                    "select",
                    Control::Select {
                        options: vec!["One".into(), "Two".into()],
                        selected: self.select,
                    },
                ),
                Node::control(
                    "list",
                    Control::ListBox {
                        items: vec!["x".into(), "y".into(), "z".into()],
                        selected: self.list,
                    },
                ),
                Node::control("date", Control::DatePicker { date: self.date }),
                Node::control("spinner", Control::Spinner { value: self.spinner, min: 0, max: 9 }),
                Node::control("separator", Control::Separator),
                Node::control("link", Control::Link { text: "More".into() }),
                Node::control("notes", Control::MultilineText { value: self.text.clone() }),
                Node::control("picture", Control::Image { image }),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        self.log.borrow_mut().push(event.clone());
        let key = |target: NodeId| target.local_key().unwrap_or_default();
        match event {
            Event::Toggled { target, on } => match key(target).as_str() {
                "check" => self.checked = on,
                "radio-a" if on => self.radio = 0,
                "radio-b" if on => self.radio = 1,
                "toggle" => self.on = on,
                _ => {}
            },
            Event::ValueChanged { target, value } => match key(target).as_str() {
                "slider" => self.slider = value,
                "spinner" => self.spinner = value,
                _ => {}
            },
            Event::SelectionChanged { target, index } => match key(target).as_str() {
                "select" => self.select = index,
                "list" => self.list = index,
                _ => {}
            },
            Event::DateChanged { date, .. } => self.date = date,
            Event::TextChanged { value, .. } => self.text = value,
            _ => {}
        }
    }
}

/// The control events `key` reported (focus moving onto it is not one).
fn events_of(log: &Log, key: &str) -> Vec<Event> {
    log.borrow()
        .iter()
        .filter(|event| event.target() == Some(NodeId::from_key(key)))
        .filter(|event| !matches!(event, Event::FocusGained { .. } | Event::FocusLost { .. }))
        .cloned()
        .collect()
}

#[test]
fn every_control_kind_is_its_gtk_widget_and_reports_once() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let mut application = Application::new(
            Controls::new(log.clone()),
            Window::new("Controls", Size::new(420, 900)),
        );
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        for (key, type_name) in [
            ("check", "GtkCheckButton"),
            ("radio-a", "GtkCheckButton"),
            ("toggle", "RnLayout"),
            ("slider", "GtkScale"),
            ("progress", "GtkProgressBar"),
            ("select", "GtkDropDown"),
            ("list", "GtkListBox"),
            ("date", "GtkMenuButton"),
            ("spinner", "GtkSpinButton"),
            ("separator", "GtkSeparator"),
            ("link", "GtkButton"),
            ("notes", "GtkScrolledWindow"),
            ("picture", "GtkPicture"),
        ] {
            assert_eq!(harness.expect(key).type_().name(), type_name, "{key}");
        }
        assert_eq!(harness.expect("link").accessible_role(), gtk::AccessibleRole::Link);

        harness.expect_as::<gtk::CheckButton>("check").set_active(true);
        harness.pump();
        assert_eq!(
            events_of(&log, "check"),
            vec![Event::Toggled { target: NodeId::from_key("check"), on: true }]
        );

        // Radios share a GTK group, and only the choice is reported.
        harness.expect_as::<gtk::CheckButton>("radio-b").set_active(true);
        harness.pump();
        assert_eq!(events_of(&log, "radio-a"), Vec::<Event>::new());
        assert_eq!(
            events_of(&log, "radio-b"),
            vec![Event::Toggled { target: NodeId::from_key("radio-b"), on: true }]
        );
        assert!(!harness.expect_as::<gtk::CheckButton>("radio-a").is_active());

        let switch =
            harness.expect("toggle").first_child().and_downcast::<gtk::Switch>().expect("a switch");
        switch.set_active(true);
        harness.pump();
        assert_eq!(events_of(&log, "toggle").len(), 1);

        harness.expect_as::<gtk::Scale>("slider").set_value(55.0);
        harness.pump();
        assert_eq!(
            events_of(&log, "slider"),
            vec![Event::ValueChanged { target: NodeId::from_key("slider"), value: 55 }]
        );

        harness.expect_as::<gtk::DropDown>("select").set_selected(1);
        harness.pump();
        assert_eq!(
            events_of(&log, "select"),
            vec![Event::SelectionChanged { target: NodeId::from_key("select"), index: Some(1) }]
        );

        let list = harness.expect_as::<gtk::ListBox>("list");
        list.select_row(list.row_at_index(2).as_ref());
        harness.pump();
        assert_eq!(
            events_of(&log, "list"),
            vec![Event::SelectionChanged { target: NodeId::from_key("list"), index: Some(2) }]
        );

        harness.expect_as::<gtk::SpinButton>("spinner").set_value(7.0);
        harness.pump();
        assert_eq!(
            events_of(&log, "spinner"),
            vec![Event::ValueChanged { target: NodeId::from_key("spinner"), value: 7 }]
        );

        let calendar = harness
            .expect_as::<gtk::MenuButton>("date")
            .popover()
            .and_then(|popover| popover.child())
            .and_downcast::<gtk::Calendar>()
            .expect("a calendar");
        calendar.select_day(&glib::DateTime::from_local(2027, 1, 2, 0, 0, 0.0).expect("a date"));
        harness.pump();
        let expected = CalendarDate::new(2027, 1, 2).expect("a date");
        assert_eq!(
            events_of(&log, "date"),
            vec![Event::DateChanged { target: NodeId::from_key("date"), date: expected }]
        );
        assert_eq!(
            harness.expect_as::<gtk::MenuButton>("date").label().as_deref(),
            Some("2027-01-02")
        );

        let view = harness
            .expect_as::<gtk::ScrolledWindow>("notes")
            .child()
            .and_downcast::<gtk::TextView>()
            .expect("a text view");
        view.buffer().set_text("two");
        harness.pump();
        assert_eq!(
            events_of(&log, "notes"),
            vec![Event::TextChanged { target: NodeId::from_key("notes"), value: "two".into() }]
        );

        assert!(harness.expect_as::<gtk::Picture>("picture").paintable().is_some());
        let progress = harness.expect_as::<gtk::ProgressBar>("progress");
        assert!((progress.fraction() - 0.4).abs() < 1e-9);
    });
}

/// A fake Settings portal answering `values` (shared, so a test can change
/// them and then announce the change).
fn settings_portal(values: Arc<Mutex<Vec<(String, String, glib::Variant)>>>) -> FakeService {
    let xml = r#"<node><interface name="org.freedesktop.portal.Settings">
        <method name="ReadOne"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
        <method name="Read"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
        <signal name="SettingChanged"><arg type="s"/><arg type="s"/><arg type="v"/></signal>
        </interface></node>"#;
    FakeService::start(
        "org.freedesktop.portal.Desktop",
        vec![Object {
            path: "/org/freedesktop/portal/desktop".into(),
            xml: xml.into(),
            handler: Arc::new(move |_, parameters| {
                let (namespace, key) =
                    parameters.get::<(String, String)>().ok_or("bad arguments")?;
                let values = values.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let (_, _, value) = values
                    .iter()
                    .find(|(n, k, _)| *n == namespace && *k == key)
                    .ok_or_else(|| format!("{namespace}.{key} not found"))?;
                Ok(glib::Variant::tuple_from_iter([glib::Variant::from_variant(value)]))
            }),
        }],
    )
}

struct Scheme;

impl Component for Scheme {
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
        Node::column("root", [Node::label("title", "Scheme"), Node::button("go", "Go")])
    }
    fn update(&mut self, _: Event) {}
}

#[test]
fn the_desktops_scheme_and_text_scale_reach_the_environment_and_follow_changes() {
    on_gtk(|| {
        let values = Arc::new(Mutex::new(vec![
            (
                "org.freedesktop.appearance".to_owned(),
                "color-scheme".to_owned(),
                1_u32.to_variant(),
            ),
            (
                "org.gnome.desktop.interface".to_owned(),
                "text-scaling-factor".to_owned(),
                1.25_f64.to_variant(),
            ),
        ]));
        let portal = settings_portal(Arc::clone(&values));
        let mut application =
            Application::new(Scheme::new(()), Window::new("Scheme", Size::new(240, 120)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let scheme = |harness: &Harness| {
            harness.with_registry(|registry| {
                registry.with_application(|application| {
                    application.environment_for(WindowId::PRIMARY, &keys::COLOR_SCHEME)
                })
            })
        };
        assert_eq!(scheme(&harness), ColorScheme::Dark);
        let settings = gtk::Settings::default().expect("GTK settings");
        assert!(
            settings.is_gtk_application_prefer_dark_theme(),
            "GTK's own widgets follow the portal"
        );
        let text_scale = harness.with_registry(|registry| {
            registry.with_application(|application| {
                application.environment_for(WindowId::PRIMARY, &keys::TEXT_SCALE).get()
            })
        });
        assert!((text_scale - 1.25).abs() < f32::EPSILON);
        // What went over the wire: the portal's own method, on its own path.
        let reads = portal.calls_to("ReadOne");
        assert!(reads.iter().all(|call| call.path == "/org/freedesktop/portal/desktop"));
        assert!(reads.iter().any(|call| {
            call.parameters.get::<(String, String)>()
                == Some(("org.freedesktop.appearance".to_owned(), "color-scheme".to_owned()))
        }));
        // GTK reads the portal for its own settings too; everything anyone
        // asked of it was a read.
        assert!(portal.calls().iter().all(|call| call.method.starts_with("Read")));

        let button = harness.expect("go");
        values.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[0].2 = 2_u32.to_variant();
        portal.emit(
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "SettingChanged",
            (
                "org.freedesktop.appearance",
                "color-scheme",
                glib::Variant::from_variant(&2_u32.to_variant()),
            )
                .to_variant(),
        );
        super::testing::pump_until("the scheme change", std::time::Duration::from_secs(3), || {
            harness.pump();
            scheme(&harness) == ColorScheme::Light
        });
        assert!(!settings.is_gtk_application_prefer_dark_theme());
        assert_eq!(harness.expect("go"), button, "the same widget, restyled");
        drop(harness);
        drop(portal);
        settings.set_gtk_application_prefer_dark_theme(false);
    });
}

/// A styled box and a translucent label.
struct Styled;

impl Component for Styled {
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
        use rustnative_core::{Color, ColumnStyle, LayoutStyle, SizeMode, VisualStyle};
        let fixed = LayoutStyle::new().width(SizeMode::Fixed(100)).height(SizeMode::Fixed(60));
        Node::column(
            "root",
            [
                Node::column_with_layout("box", [], fixed, ColumnStyle::new()).with_style(
                    VisualStyle::new()
                        .background(Color::rgb(200, 30, 30))
                        .border(Color::rgb(0, 0, 255))
                        .border_radius(20),
                ),
                Node::label("faint", "Faint").with_opacity(0.5),
            ],
        )
    }
    fn update(&mut self, _: Event) {}
}

/// The widget as GTK paints it, as RGBA rows.
fn pixels(widget: &gtk::Widget) -> (usize, usize, Vec<u8>) {
    let (width, height) = (widget.width().max(1), widget.height().max(1));
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot.to_node().expect("the widget draws something");
    let renderer = widget.native().expect("in a window").renderer().expect("a renderer");
    let texture = renderer.render_texture(&node, None);
    let (width, height) = (
        usize::try_from(texture.width()).unwrap_or(0),
        usize::try_from(texture.height()).unwrap_or(0),
    );
    let mut bytes = vec![0_u8; width * height * 4];
    let mut downloader = gtk::gdk::TextureDownloader::new(&texture);
    downloader.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (data, stride) = downloader.download_bytes();
    for row in 0..height {
        let start = row * stride;
        bytes[row * width * 4..(row + 1) * width * 4]
            .copy_from_slice(&data[start..start + width * 4]);
    }
    (width, height, bytes)
}

#[test]
fn the_linux_capability_table_is_what_gtk_paints() {
    on_gtk(|| {
        let mut application =
            Application::new(Styled::new(()), Window::new("Styled", Size::new(240, 160)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let styled = harness.expect("box");
        super::testing::pump_until(
            "the box's allocation",
            std::time::Duration::from_secs(2),
            || styled.width() > 0,
        );
        let (width, height, bytes) = pixels(&styled);
        let at = |x: usize, y: usize| {
            let offset = (y * width + x) * 4;
            (bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3])
        };
        assert_eq!(at(width / 2, height / 2), (200, 30, 30, 255), "background");
        let (_, _, blue, _) = at(width / 2, 0);
        assert!(blue > 200, "a blue border along the top edge: {:?}", at(width / 2, 0));
        let (_, _, _, corner_alpha) = at(0, 0);
        assert!(corner_alpha < 64, "a rounded corner leaves its corner unpainted: {:?}", at(0, 0));
        let faint = harness.expect("faint").opacity();
        // GTK keeps a widget's opacity as an 8-bit alpha.
        assert!((faint - 0.5).abs() <= 1.0 / 255.0, "opacity: {faint}");
        assert_eq!(
            rustnative_core::Platform::style_capabilities(&crate::LinuxPlatform::new()).backend,
            "Linux"
        );
    });
}
