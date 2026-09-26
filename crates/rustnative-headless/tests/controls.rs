//! The native controls of `rustnative_core::control` on the headless backend
//! (`PLAN.md` Milestone 48): each raises its event, the component renders
//! the new state, and a refused change leaves the control where it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use rustnative_core::{
    AccessibilityRole, CalendarDate, CheckedState, Component, Control, Event, Node, NodeId, Size,
    Window,
};
use rustnative_headless::{HeadlessApp, Query};

#[derive(Default)]
struct Settings {
    remember: bool,
    wifi: bool,
    size: usize,
    volume: i64,
    copies: i64,
    fruit: Option<usize>,
    due: CalendarDate,
    locked: bool,
    notes: String,
}

impl Component for Settings {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { due: CalendarDate::new(2026, 9, 25).unwrap(), copies: 1, ..Self::default() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "settings",
            [
                Node::checkbox("remember", "Remember me", self.remember),
                Node::toggle("wifi", "Wi-Fi", self.wifi),
                Node::radio("small", "Small", self.size == 0),
                Node::radio("large", "Large", self.size == 1),
                Node::slider("volume", self.volume, 0, 100),
                Node::spinner("copies", self.copies, 1, 9),
                Node::select("fruit", ["Apple", "Pear", "Plum"], self.fruit),
                Node::date_picker("due", self.due),
                Node::checkbox("locked", "Locked (refuses changes)", self.locked),
                Node::multiline_text("notes", self.notes.clone()),
                Node::progress("progress", Some(40)),
                Node::separator("rule"),
                Node::link("help", "Help"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        let key = |target: NodeId, key: &str| target == NodeId::from_key(key);
        match event {
            Event::Toggled { target, on } if key(target, "remember") => self.remember = on,
            Event::Toggled { target, on } if key(target, "wifi") => self.wifi = on,
            Event::Toggled { target, .. } if key(target, "small") => self.size = 0,
            Event::Toggled { target, .. } if key(target, "large") => self.size = 1,
            Event::ValueChanged { target, value } if key(target, "volume") => self.volume = value,
            Event::ValueChanged { target, value } if key(target, "copies") => self.copies = value,
            Event::SelectionChanged { index, .. } => self.fruit = index,
            Event::DateChanged { date, .. } => self.due = date,
            Event::TextChanged { value, .. } => self.notes = value,
            // `locked` ignores its own toggles: the control must stay put.
            _ => {}
        }
    }
}

fn control(app: &HeadlessApp, key: &str) -> Control {
    app.find(&Query::key(key)).unwrap().control.clone().unwrap()
}

#[test]
fn every_control_reports_its_change_and_shows_what_the_component_decided() {
    let mut app =
        HeadlessApp::launch(Window::new("Settings", Size::new(480, 900)), || Settings::new(()));

    app.toggle(&Query::key("remember")).unwrap();
    assert_eq!(
        control(&app, "remember"),
        Control::Checkbox { label: "Remember me".into(), checked: true }
    );
    app.toggle(&Query::key("wifi")).unwrap();
    assert!(matches!(control(&app, "wifi"), Control::Toggle { on: true, .. }));

    app.toggle(&Query::key("large")).unwrap();
    assert!(matches!(control(&app, "large"), Control::Radio { selected: true, .. }));
    assert!(matches!(control(&app, "small"), Control::Radio { selected: false, .. }));

    app.set_value(&Query::key("volume"), 140).unwrap();
    assert!(matches!(control(&app, "volume"), Control::Slider { value: 100, .. }), "clamped");
    app.set_value(&Query::key("copies"), 3).unwrap();
    assert!(matches!(control(&app, "copies"), Control::Spinner { value: 3, .. }));

    app.choose(&Query::key("fruit"), 2).unwrap();
    assert!(matches!(control(&app, "fruit"), Control::Select { selected: Some(2), .. }));
    assert!(app.choose(&Query::key("fruit"), 7).is_err(), "no such item");

    let date = CalendarDate::new(2027, 1, 31).unwrap();
    app.pick_date(&Query::key("due"), date).unwrap();
    assert_eq!(control(&app, "due"), Control::DatePicker { date });

    app.toggle(&Query::key("locked")).unwrap();
    assert!(matches!(control(&app, "locked"), Control::Checkbox { checked: false, .. }), "refused");
}

#[test]
fn every_control_describes_itself_to_assistive_technology() {
    let app =
        HeadlessApp::launch(Window::new("Settings", Size::new(480, 900)), || Settings::new(()));
    let node = |key| app.find(&Query::key(key)).unwrap().accessibility.clone();
    assert_eq!(node("remember").role(), AccessibilityRole::CheckBox);
    assert_eq!(node("remember").checked_state(), Some(CheckedState::Unchecked));
    assert_eq!(node("volume").role(), AccessibilityRole::Slider);
    assert_eq!(node("fruit").role(), AccessibilityRole::ComboBox);
    assert_eq!(node("copies").role(), AccessibilityRole::SpinButton);
    assert_eq!(node("rule").role(), AccessibilityRole::Separator);
    assert_eq!(node("help").role(), AccessibilityRole::Link);
    assert!(!node("progress").is_focusable());
    assert!(node("volume").is_focusable());
    // Found by role, as a test or a screen reader would.
    assert_eq!(app.find_all(&Query::role(AccessibilityRole::RadioButton)).len(), 2);
}
