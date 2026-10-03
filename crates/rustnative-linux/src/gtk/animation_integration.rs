//! Animation on real GTK widgets (Phase 5): transitions and explicit
//! animations timed by a manual clock, frames applied by the same path the
//! frame clock drives, read back from what GTK was told.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use rustnative_core::{
    AnimatedProperty, AnimatedValue, Animation, AnimationRequests, ColumnStyle, Component,
    ComponentContext, Easing, Event, LayoutStyle, ManualFrameClock, MotionPreference, Node, NodeId,
    Point, Size, SizeMode, Transition, Window, WindowId,
};

use super::testing::{Harness, on_gtk};

#[derive(Clone, PartialEq)]
struct Props {
    log: Rc<RefCell<Vec<String>>>,
    renders: Rc<Cell<u32>>,
}

struct Mover {
    props: Props,
    expanded: bool,
    faded: bool,
    animations: Option<AnimationRequests>,
}

fn fixed(width: i32, height: i32) -> LayoutStyle {
    LayoutStyle::new().width(SizeMode::Fixed(width)).height(SizeMode::Fixed(height))
}

fn linear() -> Transition {
    Transition::new(Duration::from_millis(100)).easing(Easing::Linear)
}

impl Component for Mover {
    type Props = Props;
    type Message = ();
    fn new(props: Props) -> Self {
        Self { props, expanded: false, faded: false, animations: None }
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
                Node::column_with_layout("panel", [], fixed(100, 40), ColumnStyle::new())
                    .with_transition(AnimatedProperty::Position, linear())
                    .with_transition(AnimatedProperty::Opacity, linear())
                    .with_opacity(if self.faded { 0.5 } else { 1.0 }),
                Node::button("go", "Go"),
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
            Event::Click { target } if *target == NodeId::from_key("fade") => {
                self.faded = !self.faded;
            }
            Event::Click { target } if *target == NodeId::from_key("slide") => {
                if let Some(animations) = &self.animations {
                    animations.animate(
                        "panel",
                        Animation::new(
                            AnimatedProperty::Translation,
                            AnimatedValue::Offset(Point::new(50, 0)),
                            linear(),
                        )
                        .from(AnimatedValue::Offset(Point::new(0, 0))),
                    );
                }
            }
            Event::AnimationFinished { property, .. } => {
                self.props.log.borrow_mut().push(format!("finished {property:?}"));
            }
            _ => {}
        }
    }
}

fn launch() -> (Application, Props) {
    let props = Props { log: Rc::default(), renders: Rc::default() };
    (
        Application::new(Mover::new(props.clone()), Window::new("Animation", Size::new(240, 320))),
        props,
    )
}

use rustnative_core::Application;

/// Installs a manual clock in the primary window and returns it.
fn manual_clock(harness: &Harness) -> ManualFrameClock {
    let clock = ManualFrameClock::new();
    let installed = clock.clone();
    harness.with_registry(|registry| {
        if let Some(runtime) = registry.windows.get_mut(&WindowId::PRIMARY) {
            runtime.animation.set_clock(Box::new(installed));
        }
    });
    clock
}

/// One frame at the clock's current time, as the tick callback runs it.
fn frame(harness: &Harness) {
    harness.with_registry(|registry| registry.animation_frame(WindowId::PRIMARY)).expect("a frame");
    harness.pump();
}

/// Where the panel's widget was placed, in its container.
fn panel_y(harness: &Harness) -> i32 {
    let panel = harness.expect("panel");
    let parent = panel.parent().expect("placed");
    let layout =
        parent.downcast::<super::layout_widget::RnLayout>().expect("in a framework container");
    layout.placement(&panel).expect("placed").y
}

#[test]
fn a_transition_moves_through_frames_and_ends_where_the_tree_says() {
    on_gtk(|| {
        let (mut application, props) = launch();
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let clock = manual_clock(&harness);
        let start = panel_y(&harness);
        let renders = props.renders.get();
        harness.click("go");
        assert_eq!(panel_y(&harness), start, "pinned at its start until the first frame");
        clock.advance(Duration::from_millis(50));
        frame(&harness);
        assert_eq!(panel_y(&harness), start + 40, "halfway through a linear 80 px move");
        clock.advance(Duration::from_millis(60));
        frame(&harness);
        assert_eq!(panel_y(&harness), start + 80, "at the laid-out position");
        assert_eq!(props.renders.get(), renders + 1, "frames never render: only the click did");
    });
}

#[test]
fn opacity_transitions_and_explicit_animations_report_their_end() {
    on_gtk(|| {
        let (mut application, props) = launch();
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let clock = manual_clock(&harness);
        harness.click("fade");
        clock.advance(Duration::from_millis(50));
        frame(&harness);
        let halfway = harness.expect("panel").opacity();
        assert!((halfway - 0.75).abs() < 2.0 / 255.0, "halfway: {halfway}");
        clock.advance(Duration::from_millis(60));
        frame(&harness);
        assert!((harness.expect("panel").opacity() - 0.5).abs() < 1.0 / 255.0);

        harness.click("slide");
        clock.advance(Duration::from_millis(120));
        frame(&harness);
        assert_eq!(
            *props.log.borrow(),
            vec!["finished Translation".to_owned()],
            "only the component's animation"
        );
    });
}

#[test]
fn reduced_motion_arrives_immediately() {
    on_gtk(|| {
        let (mut application, _props) = launch();
        // SAFETY: `application` outlives `harness`.
        let harness = unsafe { Harness::attach(&mut application) };
        let clock = manual_clock(&harness);
        harness.with_registry(|registry| registry.set_motion_preference(MotionPreference::Reduced));
        let start = panel_y(&harness);
        harness.click("go");
        clock.advance(Duration::from_millis(1));
        frame(&harness);
        assert_eq!(panel_y(&harness), start + 80, "no motion: straight to the end");
    });
}
