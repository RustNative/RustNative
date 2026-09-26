//! Milestone 27 native integration tests: animation against real windows.
//!
//! Frames are driven by a [`ManualFrameClock`] and a posted
//! `WM_FRAMEWORK_FRAME`, so every assertion is about an exact moment rather
//! than about whatever the machine managed in the meantime. Everything
//! else is production code: the same renderer, the same timeline, the same
//! message loop. What the tests read back is where Windows actually put the
//! control (`GetWindowRect`) and what alpha it actually has
//! (`GetLayeredWindowAttributes`) — not the framework's own bookkeeping.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use rustnative_core::{
    AnimatedProperty, AnimatedValue, Animation, AnimationRequests, ColumnStyle, Component,
    ComponentContext, Easing, Event, LayoutStyle, ManualFrameClock, MotionPreference, Node, NodeId,
    Point, Size, SizeMode, Transition, Window, WindowId,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetLayeredWindowAttributes, GetWindowLongPtrW, GetWindowRect, WS_EX_LAYERED,
};

use super::animation::{WM_FRAMEWORK_FRAME, driver};
use super::harness::NativeHarness;

type Log = Rc<RefCell<Vec<String>>>;

#[derive(Clone, PartialEq)]
struct Props {
    log: Log,
    renders: Rc<Cell<u32>>,
}

/// A panel that moves when a spacer above it grows, fades, and can be told
/// to slide by an explicit animation.
struct Mover {
    props: Props,
    expanded: bool,
    faded: bool,
    swapped: bool,
    animations: Option<AnimationRequests>,
}

fn fixed(width: i32, height: i32) -> LayoutStyle {
    LayoutStyle::new().width(SizeMode::Fixed(width)).height(SizeMode::Fixed(height))
}

/// 100 ms, linear: a duration that divides evenly for exact assertions.
fn linear() -> Transition {
    Transition::new(Duration::from_millis(100)).easing(Easing::Linear)
}

impl Component for Mover {
    type Props = Props;
    type Message = ();

    fn new(props: Props) -> Self {
        Self { props, expanded: false, faded: false, swapped: false, animations: None }
    }
    fn props(&self) -> &Props {
        &self.props
    }
    fn set_props(&mut self, props: Props) {
        self.props = props;
    }

    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::column_with_layout(
                    "spacer",
                    [],
                    fixed(100, if self.expanded { 80 } else { 0 }),
                    ColumnStyle::new(),
                ),
                // Moves when the spacer grows, and says how.
                Node::column_with_layout("panel", [], fixed(100, 40), ColumnStyle::new())
                    .with_transition(AnimatedProperty::Position, linear())
                    .with_transition(AnimatedProperty::Opacity, linear())
                    .with_opacity(if self.faded { 0.5 } else { 1.0 }),
                // One photo, shown small or large: the two nodes share an
                // identity, so the large one grows out of the small one.
                if self.swapped {
                    Node::column_with_layout("hero", [], fixed(200, 120), ColumnStyle::new())
                        .with_shared_id("photo")
                        .with_transition(AnimatedProperty::Position, linear())
                } else {
                    Node::column_with_layout("thumb", [], fixed(40, 40), ColumnStyle::new())
                        .with_shared_id("photo")
                },
                Node::button("go", "Go"),
                Node::button("swap", "Swap"),
                Node::button("fade", "Fade"),
                Node::button("slide", "Slide"),
            ],
        )
    }

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        self.props.renders.set(self.props.renders.get() + 1);
        self.animations = Some(context.animations());
        self.view()
    }

    fn update(&mut self, event: Event) {
        match &event {
            Event::Click { target } if *target == NodeId::from_key("go") => {
                self.expanded = !self.expanded;
            }
            Event::Click { target } if *target == NodeId::from_key("swap") => {
                self.swapped = !self.swapped;
            }
            Event::Click { target } if *target == NodeId::from_key("fade") => {
                self.faded = !self.faded;
            }
            Event::Click { target } if *target == NodeId::from_key("slide") => {
                if let Some(animations) = &self.animations {
                    animations.animate(
                        "panel",
                        Animation::new(
                            AnimatedProperty::Translation,
                            AnimatedValue::Offset(Point::new(60, 0)),
                            linear(),
                        )
                        .from(AnimatedValue::Offset(Point::new(0, 0))),
                    );
                }
            }
            Event::AnimationFinished { property, .. } => {
                self.props.log.borrow_mut().push(format!("finished:{property:?}"));
            }
            _ => {}
        }
    }
}

struct Fixture {
    log: Log,
    renders: Rc<Cell<u32>>,
    clock: ManualFrameClock,
}

impl Fixture {
    fn new() -> Self {
        Self { log: Log::default(), renders: Rc::new(Cell::new(0)), clock: ManualFrameClock::new() }
    }

    fn application(&self) -> rustnative_core::Application {
        rustnative_core::Application::new(
            Mover::new(Props { log: self.log.clone(), renders: self.renders.clone() }),
            Window::new("animation", Size::new(360, 420)),
        )
    }

    /// Attaches the harness with this fixture's clock installed, so no
    /// frame depends on how long anything really took.
    fn attach(&self, application: &mut rustnative_core::Application) -> NativeHarness {
        // SAFETY: every caller declares `application` before the harness,
        // so it outlives it — the same obligation as `NativeHarness::attach`.
        let mut harness = unsafe { NativeHarness::attach(application) };
        let clock = self.clock.clone();
        harness.with_runtime_mut(WindowId::PRIMARY, move |runtime| {
            runtime.animation.set_clock(Box::new(clock));
        });
        harness
    }

    /// Advances the clock and delivers one frame, exactly as the frame
    /// driver's posted message would.
    fn frame(&self, harness: &mut NativeHarness, delta: Duration) {
        self.clock.advance(delta);
        let window = harness.hwnd(WindowId::PRIMARY);
        harness.send(window, WM_FRAMEWORK_FRAME, 0, 0);
    }

    fn entries(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

fn rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live control; `rect` is exclusively borrowed.
    let read = unsafe { GetWindowRect(hwnd, &raw mut rect) } != 0;
    assert!(read, "GetWindowRect on a live control must succeed");
    rect
}

fn top_of(harness: &NativeHarness, key: &str) -> i32 {
    rect(harness.expect_control(WindowId::PRIMARY, key)).top
}

/// What Windows reports for a layered window's alpha, or `None` if the
/// window is not layered at all.
fn alpha(hwnd: HWND) -> Option<u8> {
    // SAFETY: `hwnd` is a live control; `GWL_EXSTYLE` is a documented index.
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    #[allow(clippy::cast_possible_wrap, reason = "a single fixed style bit")]
    let layered = WS_EX_LAYERED as isize;
    if style & layered == 0 {
        return None;
    }
    let mut value = 0u8;
    // SAFETY: `hwnd` carries `WS_EX_LAYERED` (just checked); the two
    // out-pointers this call does not need are null, which it documents as
    // "not requested".
    let read = unsafe {
        GetLayeredWindowAttributes(hwnd, std::ptr::null_mut(), &raw mut value, std::ptr::null_mut())
    } != 0;
    read.then_some(value)
}

/// A declared transition moves the real control through intermediate
/// positions, lands exactly where layout put it, and does not rerender the
/// component once along the way.
///
/// Catches the failure this milestone exists to prevent: animation
/// implemented as a rerender per frame. It also catches a transition that
/// never starts (the node jumps) and one that never lands (the node ends up
/// somewhere layout did not ask for).
#[test]
fn native_transition_moves_the_control_without_rerendering() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    let mut harness = fixture.attach(&mut application);
    let start = top_of(&harness, "panel");

    harness.click(WindowId::PRIMARY, "go");
    let renders_after_click = fixture.renders.get();
    assert_eq!(top_of(&harness, "panel"), start, "the panel has not moved yet: it animates");

    fixture.frame(&mut harness, Duration::from_millis(50));
    let midway = top_of(&harness, "panel");
    assert!(midway > start, "halfway through, the control is between its old and new positions");

    fixture.frame(&mut harness, Duration::from_millis(50));
    let settled = top_of(&harness, "panel");
    assert_eq!(settled - start, 80, "it lands exactly where layout put it");
    assert!(midway < settled);
    assert_eq!(
        fixture.renders.get(),
        renders_after_click,
        "not one frame caused a component render"
    );
}

/// Matched geometry (`C25`): a node arriving with the shared identity of
/// one that left starts at the leaving node's size and grows to its own.
///
/// Catches the arriving node simply appearing at its final size, which is
/// what happens when the two nodes are treated as unrelated.
#[test]
fn native_matched_geometry_grows_the_arriving_node_from_the_leaving_one() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    let mut harness = fixture.attach(&mut application);
    let width = |harness: &NativeHarness, key: &str| {
        let bounds = rect(harness.expect_control(WindowId::PRIMARY, key));
        bounds.right - bounds.left
    };
    assert_eq!(width(&harness, "thumb"), 40);

    harness.click(WindowId::PRIMARY, "swap");
    assert_eq!(width(&harness, "hero"), 40, "it starts where the thumbnail was");
    fixture.frame(&mut harness, Duration::from_millis(50));
    let midway = width(&harness, "hero");
    assert!(40 < midway && midway < 200, "halfway, it is between the two: {midway}");
    fixture.frame(&mut harness, Duration::from_millis(50));
    assert_eq!(width(&harness, "hero"), 200, "and lands at its own size");
}

/// When nothing is animating, no frames are requested; the driver's thread
/// has nothing to wake up for.
///
/// Catches an animation system that keeps posting messages forever — the
/// difference between animating and busy-waiting.
#[test]
fn native_frames_stop_when_the_animation_settles() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    let mut harness = fixture.attach(&mut application);
    let window = harness.hwnd(WindowId::PRIMARY);
    assert!(!driver::is_running(window), "an idle window asks for no frames");

    harness.click(WindowId::PRIMARY, "go");
    assert!(driver::is_running(window), "a started transition asks for frames");

    fixture.frame(&mut harness, Duration::from_millis(100));
    assert!(!driver::is_running(window), "and stops asking as soon as it settles");
    assert!(
        harness.with_runtime(WindowId::PRIMARY, |runtime| runtime.animation.timeline().is_empty())
    );
}

/// An explicit animation runs, tells the component when it finished, and
/// leaves the node back where the rendered tree puts it.
///
/// Catches an animation that never reports completion (a component waiting
/// on it would hang) and one whose final value silently disagrees with
/// layout.
#[test]
fn native_explicit_animation_reports_finishing_and_returns_to_rest() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    let mut harness = fixture.attach(&mut application);
    let resting = rect(harness.expect_control(WindowId::PRIMARY, "panel")).left;

    harness.click(WindowId::PRIMARY, "slide");
    fixture.frame(&mut harness, Duration::from_millis(50));
    let moved = rect(harness.expect_control(WindowId::PRIMARY, "panel")).left;
    assert!(moved > resting, "the node slid right");
    assert!(fixture.entries().is_empty(), "not finished yet");

    fixture.frame(&mut harness, Duration::from_millis(50));
    assert_eq!(fixture.entries(), ["finished:Translation"]);
    assert_eq!(
        rect(harness.expect_control(WindowId::PRIMARY, "panel")).left,
        resting,
        "an animation that does not fill forwards leaves the node where layout says"
    );
}

/// Opacity is realized as a real layered window with the alpha the tree
/// declares, and a transition animates it.
///
/// Catches opacity that is only bookkeeping, and a node left layered (and
/// so permanently translucent) after fading back to opaque.
#[test]
fn native_opacity_is_a_layered_window_and_animates() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    let mut harness = fixture.attach(&mut application);
    let panel = harness.expect_control(WindowId::PRIMARY, "panel");
    assert_eq!(alpha(panel), None, "a fully opaque node is not layered at all");

    harness.click(WindowId::PRIMARY, "fade");
    fixture.frame(&mut harness, Duration::from_millis(50));
    let midway = alpha(panel).expect("fading makes the node layered");
    assert!((130..=200).contains(&midway), "halfway between 255 and 128, got {midway}");

    fixture.frame(&mut harness, Duration::from_millis(50));
    assert_eq!(alpha(panel), Some(128), "0.5 opacity is alpha 128");

    harness.click(WindowId::PRIMARY, "fade");
    fixture.frame(&mut harness, Duration::from_millis(100));
    assert_eq!(alpha(panel), None, "back to opaque means back to not layered");
}

/// With reduced motion, a transition is over on its first frame: the
/// outcome without the movement.
///
/// Catches an implementation that honors the preference by *slowing* or
/// skipping the result rather than arriving at it immediately.
#[test]
fn native_reduced_motion_arrives_immediately() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    let mut harness = fixture.attach(&mut application);
    harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
        runtime.animation.set_motion_preference(MotionPreference::Reduced);
    });
    let start = top_of(&harness, "panel");

    harness.click(WindowId::PRIMARY, "go");
    fixture.frame(&mut harness, Duration::from_millis(1));
    assert_eq!(top_of(&harness, "panel") - start, 80, "already there");
    assert!(!driver::is_running(harness.hwnd(WindowId::PRIMARY)));
}

/// An animation belongs to the component that started it: when that
/// component goes away mid-flight, the animation goes with it.
///
/// Catches an animation left running against a node that no longer exists —
/// the animation equivalent of a leaked task.
#[test]
fn native_animations_end_with_the_node_they_animate() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    let mut harness = fixture.attach(&mut application);

    harness.click(WindowId::PRIMARY, "slide");
    fixture.frame(&mut harness, Duration::from_millis(20));
    assert_eq!(
        harness.with_runtime(WindowId::PRIMARY, |runtime| runtime.animation.timeline().len()),
        1
    );

    // The window closes: every node it animated is gone.
    harness.request_close(WindowId::PRIMARY);
    assert!(
        !driver::is_running(harness.hwnd(WindowId::PRIMARY)),
        "a closed window wants no frames"
    );
}
