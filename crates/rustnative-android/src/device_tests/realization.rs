//! The realized tree on a real device (Phase 1): every kind is its platform
//! view, placed where the layout engine put it; taps and typing reach
//! components; the framework never echoes; scrolling renders nothing; a
//! task on another thread wakes the main looper; measurement is the
//! host's; churn returns the census; and the shared guarantee suites hold.

use std::time::Duration;

use rustnative_conformance::host::{ConformanceHost, Driver};
use rustnative_core::{
    Alignment, Application, CalendarDate, ColumnStyle, Component, ComponentContext, Event,
    LayoutStyle, Node, NodeId, Overflow, Size, SizeMode, Window,
};

use super::harness::{Harness, Instrumentation, java, on_main};
use crate::jni_host::{Arg, Class};
use crate::units::to_px;

/// Every kind of node the mapping names.
struct Gallery {
    count: u32,
    text: String,
    changes: u32,
    on: bool,
    value: i64,
    renders: u32,
}

impl Component for Gallery {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { count: 0, text: String::new(), changes: 0, on: false, value: 3, renders: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            vec![
                Node::label("count", format!("Count: {}", self.count)),
                Node::button("increment", "Increment"),
                Node::text_input("field", self.text.clone()),
                Node::label("echo", format!("Typed: {} ({} changes)", self.text, self.changes)),
                Node::checkbox("check", "Check", self.on),
                Node::toggle("toggle", "Toggle", self.on),
                Node::radio("radio", "Radio", self.on),
                Node::slider("slider", self.value, 0, 10),
                Node::progress("progress", Some(40)),
                Node::select("select", ["One", "Two", "Three"], Some(1)),
                Node::date_picker("date", CalendarDate::new(2026, 10, 6).unwrap_or_default()),
                Node::spinner("spinner", self.value, 0, 10),
                Node::separator("separator"),
                Node::link("link", "A link"),
                Node::multiline_text("notes", "Line one\nLine two"),
                Node::label("renders", format!("{}", self.renders)),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => self.count += 1,
            Event::TextChanged { target, value } if target == NodeId::from_key("field") => {
                self.text = value;
                self.changes += 1;
            }
            Event::Toggled { target, on } if target == NodeId::from_key("check") => self.on = on,
            // The toggle reports, but this component keeps it controlled by
            // `on`: a refusal must show.
            Event::ValueChanged { target, value } if target == NodeId::from_key("slider") => {
                self.value = value;
            }
            _ => {}
        }
    }
    fn render(&mut self, _context: &mut ComponentContext<'_, ()>) -> Node {
        self.renders += 1;
        self.view()
    }
}

fn gallery() -> Application {
    Application::new(Gallery::new(()), Window::new("Gallery", Size::new(360, 800)))
}

pub(super) fn every_kind_is_its_platform_view_at_its_rectangle(_: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(gallery());
        for (key, class) in [
            ("root", "dev.rustnative.android.RnLayout"),
            ("count", "android.widget.TextView"),
            ("increment", "android.widget.Button"),
            ("field", "android.widget.EditText"),
            ("check", "android.widget.CheckBox"),
            ("toggle", "android.widget.Switch"),
            ("radio", "android.widget.RadioButton"),
            ("slider", "android.widget.SeekBar"),
            ("progress", "android.widget.ProgressBar"),
            ("select", "android.widget.Spinner"),
            ("date", "android.widget.Button"),
            ("spinner", "dev.rustnative.android.RnStepper"),
            ("separator", "android.view.View"),
            ("link", "android.widget.TextView"),
            ("notes", "android.widget.EditText"),
        ] {
            assert_eq!(harness.class_of(key), class, "`{key}` is realized as {class}");
        }
        assert_eq!(harness.text("count").as_deref(), Some("Count: 0"));
        assert_eq!(harness.text("increment").as_deref(), Some("Increment"));
        assert_eq!(harness.text("date").as_deref(), Some("2026-10-06"));
        let density = harness.density();
        for key in ["count", "increment", "field", "check", "slider", "select", "notes"] {
            let rect = harness.placed(key).unwrap_or_else(|| panic!("`{key}` was laid out"));
            let left = to_px(rect.x, density);
            let top = to_px(rect.y, density);
            let expected = [
                left,
                top,
                to_px(rect.x + rect.width, density) - left,
                to_px(rect.y + rect.height, density) - top,
            ];
            assert_eq!(
                harness.frame(key),
                expected,
                "`{key}` sits where the layout engine put it ({rect:?} dp at {density})"
            );
            assert!(rect.height > 0 && rect.width > 0, "`{key}` has a size");
        }
    });
}

pub(super) fn a_tap_reaches_update(_: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(gallery());
        harness.click("increment");
        harness.click("increment");
        assert_eq!(harness.text("count").as_deref(), Some("Count: 2"));
    });
}

pub(super) fn typing_reports_each_change_and_the_framework_never_echoes(_: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(gallery());
        let field = harness.expect("field");
        for text in ["h", "hi"] {
            let _ = crate::jni_host::call(
                &field,
                "setText",
                "(Ljava/lang/CharSequence;)V",
                &[Arg::Str(text)],
            );
            harness.pump();
        }
        assert_eq!(harness.text("echo").as_deref(), Some("Typed: hi (2 changes)"));
        // A render that sets the field to what it already shows reports
        // nothing: a tap elsewhere renders, and the count stays.
        harness.click("increment");
        assert_eq!(harness.text("echo").as_deref(), Some("Typed: hi (2 changes)"));
        assert_eq!(harness.text("field").as_deref(), Some("hi"));
    });
}

pub(super) fn controls_report_and_stay_controlled(_: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(gallery());
        harness.click("check");
        let checked = |key: &str| {
            crate::jni_host::call(&harness.expect(key), "isChecked", "()Z", &[])
                .is_ok_and(crate::jni_host::Ret::bool)
        };
        assert!(checked("check"), "the check box reported, and the component took it");
        assert!(checked("toggle"), "the switch follows the component's state");
        // The switch's own change is not the component's: it reports, the
        // component ignores it, and the switch shows the component's value.
        harness.click("toggle");
        assert!(checked("toggle"), "a refused change is undone");
        // A value the person sets on the seek bar arrives as `ValueChanged`.
        let slider = harness.expect("slider");
        let _ = crate::jni_host::call(&slider, "setProgress", "(I)V", &[Arg::Int(7)]);
        harness.pump();
        // `setProgress` from code is not the person's (`fromUser` is false),
        // so the component still has 3.
        let progress = crate::jni_host::call(&slider, "getProgress", "()I", &[])
            .map_or(-1, crate::jni_host::Ret::int);
        assert_eq!(progress, 7);
        assert_eq!(
            harness.with_application(|application| {
                let mut value = String::new();
                application.view().visit(&mut |node, _, _| {
                    if let Node::Control(control) = node {
                        if node.id() == NodeId::from_key("spinner") {
                            value = format!("{:?}", control.control());
                        }
                    }
                });
                value
            }),
            format!("{:?}", rustnative_core::Control::Spinner { value: 3, min: 0, max: 10 })
        );
    });
}

/// A long scrolling column.
struct Long;

impl Component for Long {
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
        Node::column_with_layout(
            "list",
            (0..60).map(|index| Node::label(format!("row-{index}"), format!("Row {index}"))),
            LayoutStyle::default().height(SizeMode::Fill),
            ColumnStyle::default().overflow(Overflow::Scroll),
        )
    }
    fn update(&mut self, _event: Event) {}
}

pub(super) fn scrolling_renders_nothing(_: &Instrumentation) {
    on_main(|| {
        let harness =
            Harness::launch(Application::new(Long, Window::new("Long", Size::new(360, 400))));
        assert_eq!(harness.class_of("list"), "android.widget.ScrollView");
        let before = harness.census();
        let list = harness.expect("list");
        java(
            Class::Views,
            "scrollTo",
            "(Landroid/view/View;II)V",
            &[Arg::Obj(&list), Arg::Int(0), Arg::Int(400)],
        );
        harness.pump();
        let offset =
            java(Class::Views, "scrollOffset", "(Landroid/view/View;)[I", &[Arg::Obj(&list)])
                .ints();
        assert_eq!(offset.get(1).copied(), Some(400), "the scroll view scrolled");
        assert_eq!(harness.census(), before, "no view was made or destroyed by scrolling");
    });
}

/// A component whose task finishes on another thread.
struct Waiter {
    answer: Option<u32>,
    started: bool,
}

impl Component for Waiter {
    type Props = ();
    type Message = u32;
    fn new((): ()) -> Self {
        Self { answer: None, started: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label(
            "answer",
            self.answer.map_or_else(|| "waiting".to_owned(), |answer| answer.to_string()),
        )
    }
    fn update(&mut self, _event: Event) {}
    fn message(&mut self, answer: u32) {
        self.answer = Some(answer);
    }
    fn render(&mut self, context: &mut ComponentContext<'_, u32>) -> Node {
        if !std::mem::replace(&mut self.started, true) {
            let (reply, answer) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                let _ = reply.send(42);
            });
            let _ = context.spawn(async move { answer.await.unwrap_or(0) });
        }
        self.view()
    }
}

pub(super) fn a_task_on_another_thread_wakes_the_main_looper(_: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(Application::new(
            Waiter::new(()),
            Window::new("Waiter", Size::new(200, 100)),
        ));
        harness.pump_until("the task's answer", Duration::from_secs(5), |harness| {
            harness.text("answer").as_deref() == Some("42")
        });
    });
}

/// Labels of growing length, one of them constrained.
struct Labels;

impl Component for Labels {
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
        let long = "A label long enough that it must wrap onto several lines in a narrow column";
        Node::column(
            "root",
            [
                // A column stretches its children across; these take their text's width.
                Node::label_with_layout(
                    "short",
                    "Hi",
                    LayoutStyle::default().width(SizeMode::Auto).align_self(Alignment::Start),
                ),
                Node::label_with_layout(
                    "longer",
                    "Hello, world",
                    LayoutStyle::default().width(SizeMode::Auto).align_self(Alignment::Start),
                ),
                Node::label_with_layout(
                    "wrapped",
                    long,
                    LayoutStyle::default().width(SizeMode::Fixed(120)).align_self(Alignment::Start),
                ),
                Node::label("one-line", "A label"),
            ],
        )
    }
    fn update(&mut self, _event: Event) {}
}

pub(super) fn labels_measure_with_their_text_and_wrap(_: &Instrumentation) {
    on_main(|| {
        let harness =
            Harness::launch(Application::new(Labels, Window::new("Labels", Size::new(360, 600))));
        let short = harness.placed("short").expect("laid out");
        let longer = harness.placed("longer").expect("laid out");
        assert!(longer.width > short.width, "more text is wider: {short:?} {longer:?}");
        let wrapped = harness.placed("wrapped").expect("laid out");
        let one_line = harness.placed("one-line").expect("laid out");
        assert_eq!(wrapped.width, 120);
        assert!(
            wrapped.height >= one_line.height * 3,
            "it wraps: {wrapped:?} against {one_line:?}"
        );
        // The text view draws it in as many lines as it was measured at.
        let lines = crate::jni_host::call(&harness.expect("wrapped"), "getLineCount", "()I", &[])
            .map_or(0, crate::jni_host::Ret::int);
        assert!(lines >= 3, "the view wraps too ({lines} lines)");
    });
}

/// A list whose length the test changes.
struct Churn {
    rows: u32,
}

impl Component for Churn {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { rows: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            std::iter::once(Node::button("more", "More"))
                .chain(
                    (0..self.rows)
                        .map(|index| Node::label(format!("row-{index}"), format!("{index}"))),
                )
                .collect::<Vec<_>>(),
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.rows = if self.rows >= 40 { 0 } else { self.rows + 20 };
        }
    }
}

pub(super) fn churn_returns_the_census_to_its_baseline(_: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(Application::new(
            Churn::new(()),
            Window::new("Churn", Size::new(360, 600)),
        ));
        let baseline = harness.realized();
        for _ in 0..30 {
            harness.click("more");
        }
        // 30 clicks: 20, 40, 0, … ending at 0 rows.
        let (created, destroyed) = harness.census();
        assert_eq!(harness.realized(), baseline);
        assert_eq!(
            usize::try_from(created - destroyed).unwrap_or(usize::MAX),
            baseline,
            "every view made was released"
        );
        // Ten rounds of 20 and then 40 rows: 400 rows made and released.
        assert!(created >= 400, "the churn made views ({created})");
    });
}

/// The device harness as a conformance host.
struct AndroidHost;

struct AndroidDriver<'a> {
    harness: &'a Harness,
}

impl Driver for AndroidDriver<'_> {
    fn click(&mut self, key: &str) {
        self.harness.click(key);
    }

    fn type_text(&mut self, key: &str, text: &str) {
        let field = self.harness.expect(key);
        for character in text.chars() {
            let _ = crate::jni_host::call(
                &field,
                "append",
                "(Ljava/lang/CharSequence;)V",
                &[Arg::Str(&character.to_string())],
            );
            self.harness.pump();
        }
    }

    fn scroll(&mut self, key: &str, dy: i32) {
        let view = self.harness.expect(key);
        let offset =
            java(Class::Views, "scrollOffset", "(Landroid/view/View;)[I", &[Arg::Obj(&view)])
                .ints();
        let y = offset.get(1).copied().unwrap_or(0) + to_px(dy, self.harness.density());
        java(
            Class::Views,
            "scrollTo",
            "(Landroid/view/View;II)V",
            &[Arg::Obj(&view), Arg::Int(offset.first().copied().unwrap_or(0)), Arg::Int(y)],
        );
        self.harness.pump();
    }

    fn advance(&mut self, duration: Duration) {
        let until = std::time::Instant::now() + duration;
        while std::time::Instant::now() < until {
            self.harness.pump();
            std::thread::sleep(Duration::from_millis(2));
        }
        self.harness.pump();
    }

    fn realized_objects(&self) -> usize {
        self.harness.realized()
    }
}

impl ConformanceHost for AndroidHost {
    fn name(&self) -> &'static str {
        "android"
    }

    fn run<C, F>(&mut self, window: Window, root: F, script: &mut dyn FnMut(&mut dyn Driver))
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        let harness = Harness::launch(Application::new(root(), window));
        script(&mut AndroidDriver { harness: &harness });
    }
}

pub(super) fn the_shared_guarantee_suites_hold_on_android(_: &Instrumentation) {
    on_main(|| rustnative_conformance::suites::all(&mut AndroidHost));
}
