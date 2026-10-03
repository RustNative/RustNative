//! The portable accessibility model as a screen reader reads it: over
//! AT-SPI2, from another connection (Phase 4).

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, Application, CheckedState, Component, Control, Event,
    Node, Size, Window,
};

use super::atspi_reader::{
    self, Accessible, Answer, RELATION_LABELLED_BY, Request, STATE_CHECKED, STATE_INDETERMINATE,
    STATE_REQUIRED,
};
use super::testing::{Harness, on_gtk};

type Log = Rc<RefCell<Vec<Event>>>;

struct Form {
    log: Log,
    volume: i64,
}

impl Component for Form {
    type Props = Log;
    type Message = ();
    fn new(log: Log) -> Self {
        Self { log, volume: 30 }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        Node::column(
            "form",
            [
                Node::label("title", "Account").with_accessibility(AccessibilityInfo::new(
                    AccessibilityRole::Heading { level: 2 },
                )),
                Node::label("caption", "Email address"),
                Node::text_input("email", "").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput)
                        .labelled_by("caption")
                        .description("Where receipts go")
                        .required(true)
                        .automation_id("email-field")
                        .focusable(true),
                ),
                Node::control(
                    "remember",
                    Control::Checkbox { label: "Remember me".into(), checked: true },
                ),
                Node::control("volume", Control::Slider { value: self.volume, min: 0, max: 100 }),
                Node::column("stars", []).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Slider)
                        .name("Rating")
                        .range(0.0, 5.0, 3.0, 1.0)
                        .checked(CheckedState::Mixed)
                        .focusable(true),
                ),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::ValueChanged { value, .. } = event {
            self.volume = value;
        }
        self.log.borrow_mut().push(event);
    }
}

fn dump(accessible: &Accessible, depth: usize, out: &mut String) {
    use std::fmt::Write as _;
    let _ = writeln!(
        out,
        "{}{} {:?} id={:?} desc={:?} states={:?} relations={:?}",
        "  ".repeat(depth),
        accessible.role,
        accessible.name,
        accessible.id,
        accessible.description,
        accessible.states,
        accessible.relations
    );
    for child in &accessible.children {
        dump(child, depth + 1, out);
    }
}

#[test]
fn a_form_reads_over_atspi_as_the_portable_model_describes_it() {
    on_gtk(|| {
        let log: Log = Rc::default();
        let mut application =
            Application::new(Form::new(log.clone()), Window::new("A11y form", Size::new(360, 360)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let window = atspi_reader::window("A11y form");
        let mut text = String::new();
        dump(&window, 0, &mut text);
        eprintln!("{text}");

        let heading = window.named("Account").expect("the heading");
        assert_eq!(heading.role, "heading");
        let email = window
            .find(&|accessible| accessible.role == "text box" && accessible.name == "Email address")
            .expect("the field, named by its label");
        // The automation id is the widget's buildable id, which GTK reports
        // as `AccessibleId` from the versions that implement it (4.14's
        // answer is an empty stub: see `docs/linux/accessibility.md`).
        {
            use gtk::prelude::*;
            assert_eq!(harness.expect("email").buildable_id().as_deref(), Some("email-field"));
        }
        if !email.id.is_empty() {
            assert_eq!(email.id, "email-field");
        }
        assert_eq!(email.description, "Where receipts go");
        assert!(email.has_state(STATE_REQUIRED), "required");
        let caption = window
            .named("Email address")
            .filter(|accessible| accessible.role == "label")
            .map(|a| a.path.clone());
        assert!(
            email.relations.iter().any(|(kind, targets)| *kind == RELATION_LABELLED_BY
                && caption.as_ref().is_some_and(|c| targets.contains(c))),
            "labelled by the caption: {:?}",
            email.relations
        );
        let remember = window.named("Remember me").expect("the check box");
        assert_eq!(remember.role, "checkbox");
        assert!(remember.has_state(STATE_CHECKED));
        let stars = window.named("Rating").expect("the custom slider");
        assert_eq!(stars.role, "slider");
        assert!(stars.has_state(STATE_INDETERMINATE), "a mixed check state");
        let Answer::Value(value) =
            atspi_reader::ask(Request::Value { bus: stars.bus.clone(), path: stars.path.clone() })
        else {
            unreachable!()
        };
        assert_eq!(value, Some(3.0), "the custom range's value");

        // A screen reader setting the native slider's value reaches the
        // component as the person's change.
        let volume = window
            .find(&|accessible| accessible.role == "slider" && accessible.name != "Rating")
            .expect("the volume slider");
        let Answer::Done(set) = atspi_reader::ask(Request::SetValue {
            bus: volume.bus.clone(),
            path: volume.path.clone(),
            value: 70.0,
        }) else {
            unreachable!()
        };
        assert!(set, "the Value interface accepted the change");
        harness.pump();
        assert!(
            log.borrow().iter().any(|event| matches!(event, Event::ValueChanged { value: 70, .. })),
            "{:?}",
            log.borrow()
        );
    });
}

/// A chart with virtual bars, a custom button, and a live status line.
struct Chart {
    log: Log,
    clicks: u32,
}

impl Component for Chart {
    type Props = Log;
    type Message = ();
    fn new(log: Log) -> Self {
        Self { log, clicks: 0 }
    }
    fn props(&self) -> &Log {
        &self.log
    }
    fn set_props(&mut self, log: Log) {
        self.log = log;
    }
    fn view(&self) -> Node {
        use rustnative_core::{
            AccessibleActionKind, LayoutStyle, LiveRegion, Rect, SizeMode, VirtualElement,
        };
        let bars =
            [("jan", "January", 10.0), ("feb", "February", 20.0)].into_iter().enumerate().fold(
                AccessibilityInfo::new(AccessibilityRole::Canvas).name("Sales"),
                |info, (index, (key, name, value))| {
                    let x = i32::try_from(index).unwrap_or(0) * 50;
                    info.element(VirtualElement::new(
                        key,
                        AccessibilityInfo::new(AccessibilityRole::Slider)
                            .name(name)
                            .range(0.0, 100.0, value, 1.0),
                        Rect::new(x, 0, 40, 60),
                    ))
                },
            );
        let fixed = LayoutStyle::new().width(SizeMode::Fixed(120)).height(SizeMode::Fixed(60));
        Node::column(
            "root",
            [
                Node::column_with_layout("chart", [], fixed, rustnative_core::ColumnStyle::new())
                    .with_accessibility(bars),
                Node::column("go", []).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Button)
                        .name("Go")
                        .action(AccessibleActionKind::Invoke)
                        .focusable(true),
                ),
                Node::label("status", format!("{} clicks", self.clicks)).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Status).live(LiveRegion::Polite),
                ),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::AccessibilityAction { .. }) {
            self.clicks += 1;
        }
        self.log.borrow_mut().push(event);
    }
}

#[test]
fn virtual_elements_custom_actions_and_live_regions_reach_atspi() {
    on_gtk(|| {
        use rustnative_core::{AccessibleAction, NodeId};
        let log: Log = Rc::default();
        let mut application = Application::new(
            Chart::new(log.clone()),
            Window::new("A11y chart", Size::new(320, 240)),
        );
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let window = atspi_reader::window("A11y chart");
        let chart = window.named("Sales").expect("the chart");
        let names: Vec<&str> = chart.children.iter().map(|child| child.name.as_str()).collect();
        assert_eq!(names, ["January", "February"], "the bars are the chart's accessible children");
        assert!(chart.children.iter().all(|child| child.role == "slider"));

        // A screen reader moving a bar reaches the component, naming the bar.
        let february = &chart.children[1];
        let Answer::Done(set) = atspi_reader::ask(Request::SetValue {
            bus: february.bus.clone(),
            path: february.path.clone(),
            value: 25.0,
        }) else {
            unreachable!()
        };
        assert!(set);
        harness.pump();
        assert!(log.borrow().iter().any(|event| matches!(
            event,
            Event::AccessibilityAction { element: Some(element), action: AccessibleAction::SetRangeValue(value), .. }
                if *element == NodeId::from_key("feb") && (value.get() - 25.0).abs() < f32::EPSILON
        )), "{:?}", log.borrow());

        // Invoke on a custom control is AT-SPI's "activate" action; the
        // status line is a polite live region, announced when it changes.
        let go = window.named("Go").expect("the custom button");
        let Answer::Actions(actions) =
            atspi_reader::ask(Request::Actions { bus: go.bus.clone(), path: go.path.clone() })
        else {
            unreachable!()
        };
        let index = actions
            .iter()
            .position(|name| name == "activate")
            .unwrap_or_else(|| panic!("an activate action among {actions:?}"));
        let announcements = atspi_reader::listen("Announcement", || {
            let Answer::Done(done) = atspi_reader::ask(Request::DoAction {
                bus: go.bus.clone(),
                path: go.path.clone(),
                index: i32::try_from(index).unwrap_or(0),
            }) else {
                unreachable!()
            };
            assert!(done, "the action ran");
            harness.pump();
        });
        assert!(log.borrow().iter().any(|event| matches!(
            event,
            Event::AccessibilityAction { target, element: None, action: AccessibleAction::Invoke } if *target == NodeId::from_key("go")
        )), "{:?}", log.borrow());
        assert!(
            announcements.iter().any(|announcement| announcement.contains("clicks")),
            "the live region was announced: {announcements:?}"
        );
    });
}

#[test]
fn a_removed_node_leaves_the_accessible_tree() {
    struct Toggle {
        shown: bool,
    }
    impl Component for Toggle {
        type Props = ();
        type Message = ();
        fn new((): ()) -> Self {
            Self { shown: true }
        }
        fn props(&self) -> &() {
            &()
        }
        fn set_props(&mut self, (): ()) {}
        fn view(&self) -> Node {
            let mut children = vec![Node::button("hide", "Hide")];
            if self.shown {
                children.push(Node::label("note", "Transient note"));
            }
            Node::column("root", children)
        }
        fn update(&mut self, event: Event) {
            if matches!(event, Event::Click { .. }) {
                self.shown = false;
            }
        }
    }
    on_gtk(|| {
        let mut application =
            Application::new(Toggle::new(()), Window::new("A11y removal", Size::new(240, 120)));
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        assert!(atspi_reader::window("A11y removal").named("Transient note").is_some());
        harness.click("hide");
        assert!(atspi_reader::window("A11y removal").named("Transient note").is_none());
    });
}
