//! Accessibility on a real device (Phase 4), read the way TalkBack reads
//! it: through `UiAutomation`'s `AccessibilityNodeInfo` tree, with the
//! actions a screen reader performs.

use std::time::Duration;

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, AccessibleAction, AccessibleActionKind, Application,
    Color, Component, DrawList, Event, LayoutStyle, LiveRegion, Node, NodeId, Paint, Rect, RectF,
    Size, SizeMode, VirtualElement, Window,
};

use super::harness::{Harness, Instrumentation, keep, on_main, with_kept};

/// A settings form with every kind of annotation.
struct Form {
    wifi: bool,
    volume: i64,
    pressed: u32,
    element: Option<String>,
}

impl Component for Form {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { wifi: true, volume: 4, pressed: 0, element: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let chart = DrawList::new()
            .fill_rect(RectF::new(0.0, 0.0, 50.0, 80.0), Paint::color(Color::rgb(30, 120, 200)))
            .fill_rect(RectF::new(60.0, 20.0, 50.0, 60.0), Paint::color(Color::rgb(200, 120, 30)));
        Node::column(
            "root",
            vec![
                Node::label("title", "Settings").with_accessibility(AccessibilityInfo::new(
                    AccessibilityRole::Heading { level: 1 },
                )),
                Node::checkbox("wifi", "Wi-Fi", self.wifi),
                Node::slider("volume", self.volume, 0, 10),
                Node::label("name-label", "Your name"),
                Node::text_input("name", "").with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput).labelled_by("name-label"),
                ),
                Node::column(
                    "custom",
                    [Node::label("custom-text", format!("Pressed {}", self.pressed))],
                )
                .with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Button)
                        .name("Custom action")
                        .automation_id("custom")
                        .action(AccessibleActionKind::Invoke)
                        .focusable(true),
                ),
                Node::label(
                    "status",
                    format!("Element {}", self.element.as_deref().unwrap_or("none")),
                )
                .with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Status).live(LiveRegion::Polite),
                ),
                Node::canvas("chart", chart, LayoutStyle::default().height(SizeMode::Fixed(90)))
                    .with_accessibility(
                        AccessibilityInfo::new(AccessibilityRole::Canvas)
                            .name("Chart")
                            .element(VirtualElement::new(
                                "bar-a",
                                AccessibilityInfo::new(AccessibilityRole::Image)
                                    .name("Bar A")
                                    .action(AccessibleActionKind::Invoke),
                                Rect::new(0, 0, 50, 80),
                            ))
                            .element(VirtualElement::new(
                                "bar-b",
                                AccessibilityInfo::new(AccessibilityRole::Image)
                                    .name("Bar B")
                                    .action(AccessibleActionKind::Invoke),
                                Rect::new(60, 20, 50, 60),
                            )),
                    ),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Toggled { target, on } if target == NodeId::from_key("wifi") => self.wifi = on,
            Event::ValueChanged { target, value } if target == NodeId::from_key("volume") => {
                self.volume = value;
            }
            Event::AccessibilityAction { target, element, action: AccessibleAction::Invoke } => {
                if target == NodeId::from_key("custom") {
                    self.pressed += 1;
                } else if let Some(element) = element {
                    self.element = element.local_key();
                }
            }
            _ => {}
        }
    }
}

/// One node of the dumped tree.
#[derive(Debug, Clone)]
struct A11y {
    class: String,
    text: String,
    description: String,
    id: String,
    flags: String,
    range: String,
    labelled_by: String,
}

fn parse(dump: &str) -> Vec<A11y> {
    dump.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            (fields.len() >= 11).then(|| A11y {
                class: fields[1].to_owned(),
                text: fields[2].to_owned(),
                description: fields[3].to_owned(),
                id: fields[4].to_owned(),
                flags: fields[5].to_owned(),
                range: fields[6].to_owned(),
                labelled_by: fields[8].to_owned(),
            })
        })
        .collect()
}

fn find<'a>(nodes: &'a [A11y], what: &str) -> &'a A11y {
    nodes
        .iter()
        .find(|node| node.id == what || node.text == what || node.description == what)
        .unwrap_or_else(|| panic!("no node `{what}` in the tree:\n{nodes:#?}"))
}

fn launch_form() {
    on_main(|| {
        keep(Harness::launch(Application::new(
            Form::new(()),
            Window::new("Form", Size::new(360, 700)),
        )));
    });
}

fn label(key: &'static str) -> String {
    on_main(move || with_kept(|harness| harness.text(key).unwrap_or_default()))
}

/// Every element of the tree a screen reader sees.
fn the_tree(instrumentation: &Instrumentation) -> Vec<A11y> {
    instrumentation.wait_idle();
    parse(&instrumentation.dump_accessibility())
}

pub(super) fn the_tree_carries_the_portable_model(instrumentation: &Instrumentation) {
    launch_form();
    let tree = the_tree(instrumentation);
    let title = find(&tree, "Settings");
    assert!(title.flags.contains('h'), "the title is a heading: {title:?}");
    let wifi = find(&tree, "Wi-Fi");
    assert_eq!(wifi.class, "android.widget.CheckBox");
    assert!(
        wifi.flags.contains('c') && wifi.flags.contains('C'),
        "checkable and checked: {wifi:?}"
    );
    let volume =
        tree.iter().find(|node| node.class == "android.widget.SeekBar").expect("the slider");
    assert!(volume.range.starts_with("0.0:10.0:4.0"), "the range: {volume:?}");
    let name = tree.iter().find(|node| node.id == "name").expect("the field, found by its key");
    assert_eq!(name.labelled_by, "Your name", "labelled by its label: {name:?}");
    let custom = find(&tree, "custom");
    assert_eq!(custom.class, "android.widget.Button");
    assert_eq!(custom.description, "Custom action");
    assert!(
        custom.flags.contains('k'),
        "a container with an Invoke action is clickable: {custom:?}"
    );
    let bar = find(&tree, "Bar A");
    assert_eq!(bar.class, "android.widget.ImageView", "a virtual element, with its role: {bar:?}");
    assert!(tree.iter().any(|node| node.description == "Bar B"));
}

pub(super) fn a_screen_readers_actions_reach_the_component(instrumentation: &Instrumentation) {
    launch_form();
    instrumentation.wait_idle();
    // ACTION_CLICK (16) on the custom control: the portable Invoke.
    assert!(
        instrumentation.perform_accessibility_action("custom", 16, 0.0),
        "the custom control took the click"
    );
    instrumentation
        .wait_for("the invoke", Duration::from_secs(5), || label("custom-text") == "Pressed 1");
    // ACTION_CLICK on a virtual element: Invoke with the element.
    assert!(
        instrumentation.perform_accessibility_action("Bar B", 16, 0.0),
        "the element took the click"
    );
    instrumentation.wait_for("the element's invoke", Duration::from_secs(5), || {
        label("status") == "Element bar-b"
    });
    // ACTION_SET_PROGRESS on the native seek bar: its own action, reported
    // as the person's value.
    assert!(
        instrumentation.perform_accessibility_action("volume", android_set_progress(), 7.0),
        "the slider took the value"
    );
    instrumentation.wait_for("the slider's value", Duration::from_secs(5), || {
        on_main(|| {
            with_kept(|harness| {
                harness.with_application(|application| {
                    format!("{:?}", application.view()).contains("Slider { value: 7")
                })
            })
        })
    });
}

/// `android.R.id.accessibilityActionSetProgress`.
const fn android_set_progress() -> i32 {
    0x0102_003d
}

pub(super) fn the_tree_is_served_to_a_running_talkback(instrumentation: &Instrumentation) {
    const TALKBACK: &str =
        "com.google.android.marvin.talkback/com.google.android.marvin.talkback.TalkBackService";
    let installed = instrumentation.shell("pm list packages com.google.android.marvin.talkback");
    assert!(installed.contains("talkback"), "TalkBack is installed on this device");
    let previous = instrumentation
        .shell("settings get secure enabled_accessibility_services")
        .trim()
        .to_owned();
    let _ = instrumentation
        .shell(&format!("settings put secure enabled_accessibility_services {TALKBACK}"));
    let _ = instrumentation.shell("settings put secure accessibility_enabled 1");
    let restore = |instrumentation: &Instrumentation| {
        let value = if previous.is_empty() || previous == "null" {
            String::new()
        } else {
            previous.clone()
        };
        if value.is_empty() {
            let _ = instrumentation.shell("settings delete secure enabled_accessibility_services");
        } else {
            let _ = instrumentation
                .shell(&format!("settings put secure enabled_accessibility_services {value}"));
        }
    };
    let running = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !instrumentation.touch_exploration() {
            assert!(
                std::time::Instant::now() < deadline,
                "TalkBack did not start exploring by touch"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        // TalkBack opens its own tutorial when it starts: the form comes
        // back in front of it.
        std::thread::sleep(Duration::from_secs(2));
        instrumentation.bring_to_front();
        launch_form();
        let tree = the_tree(instrumentation);
        assert!(find(&tree, "Settings").flags.contains('h'));
        // ACTION_ACCESSIBILITY_FOCUS (64) on a virtual element: the
        // provider moves TalkBack's focus there. TalkBack moves focus itself
        // as windows change, so the focus is asked for until it holds.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                instrumentation.perform_accessibility_action("Bar A", 64, 0.0),
                "Bar A took accessibility focus"
            );
            std::thread::sleep(Duration::from_millis(300));
            if find(&the_tree(instrumentation), "Bar A").flags.contains('a') {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "Bar A never held TalkBack's focus");
        }
    }));
    restore(instrumentation);
    if let Err(payload) = running {
        std::panic::resume_unwind(payload);
    }
}
