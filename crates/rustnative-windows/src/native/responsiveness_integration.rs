//! Responsiveness on the native backend (`PLAN.md` Milestone 54): a hidden
//! screen's periodic task stops and resumes when shown, and an idle window
//! with nothing to do receives no wakes at all.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use rustnative_core::{
    Application, Component, ComponentContext, Event, Node, NodeId, Size, SuspendRule, TaskScope,
    Window, WindowId,
};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_TIMER;

use super::harness::NativeHarness;

static TICKS: AtomicU32 = AtomicU32::new(0);

struct Ticker {
    scope: Option<TaskScope>,
}

impl Ticker {
    fn arm(&self) {
        if let Some(scope) = &self.scope {
            let scheduler = scope.scheduler().clone();
            scope.spawn_with(SuspendRule::Defer, async move {
                scheduler.sleep(Duration::from_millis(100)).await;
            });
        }
    }
}

impl Component for Ticker {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { scope: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("ticker", "ticking")
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, (): ()) {
        TICKS.fetch_add(1, Ordering::SeqCst);
        self.arm();
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if self.scope.is_none() {
            self.scope = Some(context.task_scope());
            self.arm();
        }
        self.view()
    }
}

struct Screens {
    hidden: bool,
}

impl Component for Screens {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { hidden: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("screens", [])
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("toggle")) {
            self.hidden = !self.hidden;
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let ticker = context.child::<Ticker>("ticker").hidden(self.hidden);
        Node::column("screens", [Node::button("toggle", "Toggle"), ticker])
    }
}

/// Pumps for `duration` of real time, returning every message handled.
fn pump_for(harness: &mut NativeHarness, duration: Duration) -> Vec<u32> {
    let deadline = Instant::now() + duration;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        seen.extend(harness.pump_counting());
        std::thread::sleep(Duration::from_millis(5));
    }
    seen
}

#[test]
fn a_hidden_screen_stops_its_periodic_work_and_an_idle_window_is_not_woken() {
    let mut application =
        Application::new(Screens::new(()), Window::new("responsive", Size::new(300, 200)));
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    pump_for(&mut harness, Duration::from_millis(600));
    let shown = TICKS.load(Ordering::SeqCst);
    assert!(shown >= 3, "ticking while shown: {shown}");

    harness.click(WindowId::PRIMARY, "toggle");
    pump_for(&mut harness, Duration::from_millis(200)); // the tick in flight
    let at_hide = TICKS.load(Ordering::SeqCst);
    // Idle: the only periodic work is suspended, so nothing wakes the
    // window — no timer, no scheduler wake, nothing — for two seconds.
    let idle = pump_for(&mut harness, Duration::from_secs(2));
    assert_eq!(TICKS.load(Ordering::SeqCst), at_hide, "no ticks while hidden");
    assert!(!idle.contains(&WM_TIMER), "no timer wakes while idle: {idle:?}");
    assert!(idle.is_empty(), "no wakes of any kind while idle: {idle:?}");

    harness.click(WindowId::PRIMARY, "toggle");
    pump_for(&mut harness, Duration::from_millis(600));
    assert!(TICKS.load(Ordering::SeqCst) >= at_hide + 3, "ticking again once shown");
}
