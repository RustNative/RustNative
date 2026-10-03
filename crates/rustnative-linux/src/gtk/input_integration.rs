//! Input on real GTK windows (Phase 3): shortcuts, keys to the focused
//! node, focus, pointer and wheel routing, gestures, hover, cursors, drops,
//! the clipboard, and input-method composition.
//!
//! Samples enter where GDK's own events enter the backend — the window's
//! controllers' handlers, as `Raw` samples and `Work` items — because no
//! compositor lets a test inject input into a Wayland client. Under X11,
//! `real_x11_input_reaches_the_component` additionally drives the XTest
//! extension, which is real input from the X server.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use rustnative_core::{
    Application, ClipboardAction, Command, CommandId, Component, ComponentContext, Composition,
    Cursor, DragData, DropEffect, Event, InputInterest, InputRequests, KeyCode, KeyModifiers,
    LayoutStyle, Node, NodeId, PointerButton, PointerButtons, PointerKind, PointerPhase, Shortcut,
    Size, SizeMode, WheelDelta, Window, WindowId,
};

use super::backend::{Work, post};
use super::input::pointer::Raw;
use super::testing::{Harness, on_gtk, pump_until};

const SAVE: CommandId = CommandId::new("test.save");

type Log = Rc<RefCell<Vec<Event>>>;

struct Surface {
    log: Log,
    input: Option<InputRequests>,
}

impl Component for Surface {
    type Props = Log;
    type Message = ();
    fn new(log: Log) -> Self {
        Self { log, input: None }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        Node::column("root", [])
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::DragEnter { .. } | Event::DragOver { .. }) {
            if let Some(input) = &self.input {
                input.set_drop_effect(DropEffect::Copy);
            }
        }
        self.log.borrow_mut().push(event);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.input = Some(context.input());
        context
            .command(Command::new(SAVE, "Save").shortcut(Shortcut::ctrl(KeyCode::Character('s'))));
        let pad = LayoutStyle::new().width(SizeMode::Fixed(120)).height(SizeMode::Fixed(80));
        Node::column(
            "root",
            [
                Node::text_input("field", ""),
                Node::column_with_layout("pad", [], pad, rustnative_core::ColumnStyle::new())
                    .with_input(InputInterest::new().pointer().wheel().gestures().drop_target())
                    .with_cursor(Cursor::Pointer),
                Node::column("custom", []).with_accessibility(
                    rustnative_core::AccessibilityInfo::new(
                        rustnative_core::AccessibilityRole::Group,
                    )
                    .name("Custom")
                    .focusable(true),
                ),
            ],
        )
    }
}

fn launch(log: &Log) -> (Application, Log) {
    (
        Application::new(Surface::new(log.clone()), Window::new("Input", Size::new(320, 240))),
        log.clone(),
    )
}

fn events(log: &Log, matching: impl Fn(&Event) -> bool) -> Vec<Event> {
    log.borrow().iter().filter(|event| matching(event)).cloned().collect()
}

/// The pad's origin in the window root's coordinates.
fn pad_origin(harness: &Harness) -> (f64, f64) {
    let pad = harness.expect("pad");
    let root = harness.with_registry(|registry| registry.windows[&WindowId::PRIMARY].root.clone());
    let point = pad.compute_point(&root, &gtk::graphene::Point::new(0.0, 0.0)).expect("same tree");
    (f64::from(point.x()), f64::from(point.y()))
}

fn sample(
    phase: PointerPhase,
    x: f64,
    y: f64,
    button: Option<PointerButton>,
    buttons: PointerButtons,
) -> Raw {
    Raw::Sample {
        phase,
        pointer: super::input::pointer::MOUSE,
        sequence: None,
        kind: PointerKind::Mouse,
        x,
        y,
        button,
        buttons,
        modifiers: KeyModifiers::default(),
        pressure: None,
    }
}

#[test]
fn a_shortcut_reaches_its_command_before_the_focused_field() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let (mut application, log) = launch(&log);
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        harness.expect("field").grab_focus();
        harness.pump();
        let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
        let taken = super::backend::answer(WindowId::PRIMARY, |registry| {
            registry.shortcut(WindowId::PRIMARY, KeyCode::Character('S'), ctrl)
        });
        assert_eq!(taken, Some(true), "Ctrl+S is the command's");
        assert_eq!(
            events(&log, |event| matches!(event, Event::Command { .. })),
            vec![Event::Command { id: SAVE }]
        );
        let other = super::backend::answer(WindowId::PRIMARY, |registry| {
            registry.shortcut(WindowId::PRIMARY, KeyCode::Character('Q'), ctrl)
        });
        assert_eq!(other, Some(false), "Ctrl+Q is nobody's");
    });
}

#[test]
fn focus_and_keys_go_to_the_focused_node() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let (mut application, log) = launch(&log);
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let window = harness.gtk_window(WindowId::PRIMARY).expect("a window");
        window.present();
        harness.expect("field").grab_focus();
        pump_until("focus on the field", Duration::from_secs(2), || {
            harness.pump();
            !events(&log, |event| matches!(event, Event::FocusGained { .. })).is_empty()
        });
        let field = NodeId::from_key("field");
        assert_eq!(
            events(&log, |event| matches!(event, Event::FocusGained { .. })),
            vec![Event::FocusGained { target: field }]
        );
        post(Work::Key {
            window: WindowId::PRIMARY,
            key: KeyCode::Enter,
            modifiers: KeyModifiers::default(),
            pressed: true,
        });
        harness.pump();
        assert!(log.borrow().contains(&Event::KeyDown {
            target: Some(field),
            key: KeyCode::Enter,
            modifiers: KeyModifiers::default()
        }));

        // A focusable container takes focus, loses it the field's way, and
        // gets an input method: committed text and compositions reach it.
        let custom = NodeId::from_key("custom");
        let custom_widget = harness.expect("custom");
        assert!(custom_widget.is_focusable(), "a focusable container is a GTK focus stop");
        custom_widget.grab_focus();
        pump_until("focus on the container", Duration::from_secs(2), || {
            harness.pump();
            log.borrow().contains(&Event::FocusGained { target: custom })
        });
        assert!(log.borrow().contains(&Event::FocusLost { target: field }));
        post(Work::Composition { window: WindowId::PRIMARY, composition: Composition::Started });
        post(Work::Composition {
            window: WindowId::PRIMARY,
            composition: Composition::Updated { text: "ni".into(), cursor: 2 },
        });
        post(Work::Text { window: WindowId::PRIMARY, text: "你".into() });
        post(Work::Text { window: WindowId::PRIMARY, text: "a".into() });
        harness.pump();
        let compositions: Vec<Event> = events(&log, |event| {
            matches!(event, Event::Composition { .. } | Event::TextInput { .. })
        });
        assert_eq!(
            compositions,
            vec![
                Event::Composition { target: Some(custom), composition: Composition::Started },
                Event::Composition {
                    target: Some(custom),
                    composition: Composition::Updated { text: "ni".into(), cursor: 2 }
                },
                Event::Composition {
                    target: Some(custom),
                    composition: Composition::Committed { text: "你".into() }
                },
                Event::TextInput { target: Some(custom), text: "a".into() },
            ]
        );
    });
}

#[test]
fn pointer_samples_reach_the_interested_node_in_its_coordinates() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let (mut application, log) = launch(&log);
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let (x, y) = pad_origin(&harness);
        let pad = NodeId::from_key("pad");
        let primary = PointerButtons::none().with(PointerButton::Primary);
        let route = |raw: Raw| {
            super::backend::answer(WindowId::PRIMARY, |registry| {
                registry.pointer(WindowId::PRIMARY, raw)
            })
        };
        route(sample(PointerPhase::Move, x + 10.0, y + 10.0, None, PointerButtons::none()));
        route(sample(
            PointerPhase::Down,
            x + 10.0,
            y + 10.0,
            Some(PointerButton::Primary),
            primary,
        ));
        route(sample(
            PointerPhase::Up,
            x + 11.0,
            y + 10.0,
            Some(PointerButton::Primary),
            PointerButtons::none(),
        ));
        harness.pump();
        assert!(log.borrow().contains(&Event::PointerEnter { target: pad }), "hover began");
        let down = events(&log, |event| matches!(event, Event::PointerDown { .. }));
        let [Event::PointerDown { target, pointer }] = down.as_slice() else {
            panic!("one press: {down:?}")
        };
        assert_eq!(*target, pad);
        assert_eq!((pointer.position().x, pointer.position().y), (10, 10), "local coordinates");
        assert_eq!(events(&log, |event| matches!(event, Event::PointerUp { .. })).len(), 1);
        // A press-and-release in place is a tap.
        assert!(
            events(&log, |event| matches!(event, Event::Gesture { .. })).iter().any(|event| {
                matches!(
                    event,
                    Event::Gesture { gesture: rustnative_core::Gesture::Tap { .. }, .. }
                )
            }),
            "a tap was recognized"
        );

        // A wheel over the pad is the pad's, and GTK does not also scroll.
        let propagation = route(Raw::Scroll {
            x: x + 5.0,
            y: y + 5.0,
            delta: WheelDelta::Lines { x: 0, y: 120 },
        });
        assert_eq!(propagation, Some(gtk::glib::Propagation::Stop));
        assert!(
            log.borrow()
                .contains(&Event::Wheel { target: pad, delta: WheelDelta::Lines { x: 0, y: 120 } })
        );

        route(Raw::Left);
        harness.pump();
        assert!(log.borrow().contains(&Event::PointerLeave { target: pad }), "hover ended");
        assert_eq!(
            harness.expect("pad").cursor().and_then(|cursor| cursor.name()).as_deref(),
            Some("pointer"),
            "the declared cursor"
        );
    });
}

#[test]
fn a_drop_negotiates_through_the_component() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let (mut application, log) = launch(&log);
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let pad = NodeId::from_key("pad");
        let has_target = harness.with_registry(|registry| {
            registry.windows[&WindowId::PRIMARY]
                .renderer
                .registry
                .get(pad)
                .is_some_and(|object| object.drop.is_some())
        });
        assert!(has_target, "a drop-interested node has a GtkDropTarget");
        let data = DragData::new().with_text("hello");
        let effect = super::backend::answer(WindowId::PRIMARY, |registry| {
            registry.drag(
                WindowId::PRIMARY,
                Event::DragEnter {
                    target: pad,
                    data: data.clone(),
                    position: rustnative_core::Point::new(3, 4),
                },
            )
        });
        assert_eq!(effect, Some(DropEffect::Copy), "the component's answer");
        assert!(matches!(log.borrow().last(), Some(Event::DragEnter { .. })));
    });
}

#[test]
fn clipboard_changes_and_shortcuts_are_reported() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let (mut application, log) = launch(&log);
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let window = harness.gtk_window(WindowId::PRIMARY).expect("a window");
        window.clipboard().set_text("copied");
        pump_until("the clipboard change", Duration::from_secs(2), || {
            harness.pump();
            log.borrow().contains(&Event::ClipboardChanged { window: WindowId::PRIMARY })
        });
        let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
        post(Work::Key {
            window: WindowId::PRIMARY,
            key: KeyCode::Character('V'),
            modifiers: ctrl,
            pressed: true,
        });
        pump_until("the paste", Duration::from_secs(2), || {
            harness.pump();
            log.borrow().iter().any(|event| matches!(event, Event::Clipboard { .. }))
        });
        assert!(log.borrow().iter().any(|event| matches!(
            event,
            Event::Clipboard { action: ClipboardAction::Paste { text: Some(text) }, .. } if text == "copied"
        )));
    });
}

/// Real input through the X server's XTest extension: a click on a button
/// and text typed into a field, delivered by the X server as for a person.
#[test]
fn real_x11_input_reaches_the_component() {
    if std::env::var("RUSTNATIVE_PRIVATE_X").as_deref() != Ok("1") {
        eprintln!(
            "skipped: real input is injected through XTest, on a private X server (tools/linux-session.sh xvfb)"
        );
        return;
    }
    on_gtk(|| {
        let log: Log = Rc::default();
        let (mut application, log) = launch(&log);
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let window = harness.gtk_window(WindowId::PRIMARY).expect("a window");
        window.present();
        harness.expect("field").grab_focus();
        harness.pump();
        // With no window manager, X keyboard focus follows the pointer:
        // put it over the window.
        let surface =
            window.surface().and_downcast::<gdk4_x11::X11Surface>().expect("an X11 surface");
        let moved = std::process::Command::new("xdotool")
            .args(["mousemove", "--window", &surface.xid().to_string(), "20", "20"])
            .status();
        assert!(moved.is_ok_and(|status| status.success()), "xdotool moves the pointer");
        harness.pump();
        let typed =
            std::process::Command::new("xdotool").args(["type", "--delay", "20", "hi"]).status();
        assert!(
            typed.is_ok_and(|status| status.success()),
            "xdotool is installed (tools/linux-session.sh needs it)"
        );
        pump_until("typed text", Duration::from_secs(3), || {
            harness.pump();
            harness.expect_as::<gtk::Entry>("field").text() == "hi"
        });
        assert!(
            log.borrow()
                .iter()
                .any(|event| matches!(event, Event::TextChanged { value, .. } if value == "hi"))
        );
        assert!(
            log.borrow()
                .iter()
                .any(|event| matches!(event, Event::KeyDown { key: KeyCode::Character('H'), .. }))
        );
    });
}
