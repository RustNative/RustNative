//! Driving `rustnative_core`'s `Timeline` from `GdkFrameClock`, and
//! applying what it produces to widgets.
//!
//! ```text
//! render / relayout   the renderer notices a transitioned property changed
//!        │            and records what it moved from and to
//!        ▼
//! after_render        those become timeline transitions; a tick callback is
//!        │            added to the window root if anything now animates
//!        ▼
//! tick                GDK's frame clock, once per frame the compositor
//!        │            paces: advance the timeline, apply each value
//!        ▼
//! idle                nothing animating: the tick callback is removed
//! ```
//!
//! No step touches a component except an animation a component started
//! ending, which arrives as `Event::AnimationFinished`.

use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{
    AnimationOwner, AnimationRequest, Event, FrameClock, MotionPreference, Timeline, WindowId,
};

use super::backend::{Work, post};
use super::registry::WindowRegistry;

/// A monotonic clock for real frames, anchored at its creation.
#[derive(Debug)]
struct RealClock {
    epoch: Instant,
}

impl FrameClock for RealClock {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }
}

/// One window's animation state.
pub(crate) struct AnimationState {
    pub(crate) timeline: Timeline,
    clock: Box<dyn FrameClock>,
    tick: Option<gtk::TickCallbackId>,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self {
            timeline: Timeline::new(),
            clock: Box::new(RealClock { epoch: Instant::now() }),
            tick: None,
        }
    }
}

impl std::fmt::Debug for AnimationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationState")
            .field("active", &self.timeline.is_active())
            .finish_non_exhaustive()
    }
}

impl AnimationState {
    /// Replaces the clock frames are timed by — how a test animates
    /// without waiting.
    #[cfg(test)]
    pub(crate) fn set_clock(&mut self, clock: Box<dyn FrameClock>) {
        self.clock = clock;
    }

    /// Stops the frames for good (the window is going away).
    pub(crate) fn release(&mut self) {
        if let Some(tick) = self.tick.take() {
            tick.remove();
        }
    }
}

impl WindowRegistry {
    /// Starts the transitions the last render or layout found, forgets
    /// animations of removed nodes, and starts or stops the frames.
    pub(crate) fn after_animation_change(&mut self, window: WindowId) {
        let Some(runtime) = self.windows.get_mut(&window) else { return };
        let now = runtime.animation.clock.now();
        for request in runtime.renderer.take_transitions() {
            runtime.animation.timeline.transition(
                request.node,
                request.property,
                request.from,
                request.to,
                request.transition,
                now,
            );
        }
        for node in runtime.renderer.forgotten_nodes() {
            runtime.animation.timeline.forget(node);
        }
        self.sync_frames(window);
    }

    /// Applies the animation requests components made during the dispatch
    /// that just finished.
    pub(crate) fn apply_animation_requests(&mut self, window: WindowId) {
        let requests =
            self.with_application(|application| application.take_animation_requests(window));
        if requests.is_empty() {
            return;
        }
        let Some(runtime) = self.windows.get_mut(&window) else { return };
        let now = runtime.animation.clock.now();
        for request in requests {
            match request {
                AnimationRequest::Start { node, animation, owner } => {
                    if !runtime.renderer.snapshot.contains(node) {
                        continue;
                    }
                    let current = runtime.renderer.current_value(node, animation.property());
                    runtime.animation.timeline.start(
                        node,
                        &animation,
                        AnimationOwner::Component(owner),
                        current,
                        now,
                    );
                }
                AnimationRequest::Cancel { node, property } => {
                    if let Some(id) = runtime.animation.timeline.running_id(node, property) {
                        if let Some(frame) = runtime.animation.timeline.cancel(id) {
                            runtime.renderer.apply_animation_frame(&frame, &runtime.root);
                        }
                    }
                }
                AnimationRequest::CancelOwner(owner) => {
                    let frames =
                        runtime.animation.timeline.cancel_owner(AnimationOwner::Component(owner));
                    for frame in frames {
                        runtime.renderer.apply_animation_frame(&frame, &runtime.root);
                    }
                }
                // A request kind this backend has not caught up with leaves
                // the property where the rendered tree puts it.
                _ => {}
            }
        }
        self.sync_frames(window);
    }

    /// One frame: advance the timeline, apply what moved, tell components
    /// what ended.
    pub(crate) fn animation_frame(&mut self, window: WindowId) -> Result<(), crate::Error> {
        rustnative_core::perf::frame();
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        let now = runtime.animation.clock.now();
        let output = runtime.animation.timeline.tick(now);
        for frame in &output.frames {
            runtime.renderer.apply_animation_frame(frame, &runtime.root);
        }
        for finished in output.finished {
            // A transition ending is the backend's own business; only an
            // animation a component started is reported to it.
            if matches!(finished.owner, AnimationOwner::Component(_)) {
                self.dispatch(
                    window,
                    Event::AnimationFinished { target: finished.node, property: finished.property },
                )?;
            }
        }
        self.sync_frames(window);
        Ok(())
    }

    /// Adds or removes the window's tick callback to match whether anything
    /// is animating.
    fn sync_frames(&mut self, window: WindowId) {
        let Some(runtime) = self.windows.get_mut(&window) else { return };
        let active = runtime.animation.timeline.is_active() && !runtime.destroyed;
        match (active, runtime.animation.tick.is_some()) {
            (true, false) => {
                runtime.animation.tick = Some(runtime.root.add_tick_callback(move |_, _| {
                    post(Work::Frame(window));
                    glib::ControlFlow::Continue
                }));
            }
            (false, true) => runtime.animation.release(),
            _ => {}
        }
    }

    /// Follows the desktop's reduced-motion setting in every window.
    pub(crate) fn set_motion_preference(&mut self, motion: MotionPreference) {
        for runtime in self.windows.values_mut() {
            runtime.animation.timeline.set_motion_preference(motion);
        }
    }
}
