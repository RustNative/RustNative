//! Driving `rustnative_core`'s [`Timeline`] from real frames, and applying
//! what it produces to native objects.
//!
//! # The loop
//!
//! ```text
//! render / relayout      the renderer notices a transitioned property changed
//!        │               and records what it moved from and to
//!        ▼
//! after_render           those become timeline transitions; the frame driver
//!        │               is started if anything is now animating
//!        ▼
//! WM_FRAMEWORK_FRAME     the driver's posted message: tick the timeline and
//!        │               apply each value to its native object
//!        ▼
//! idle                   nothing animating: the driver stops, the thread sleeps
//! ```
//!
//! No step of that touches a component. A frame changes native properties
//! and nothing else — the declarative tree it was started from is still the
//! tree, and `PLAN.md`'s rule that animation must not rerender holds by
//! construction rather than by care.
//!
//! The one thing that *does* reach a component is an animation ending,
//! which arrives as an ordinary `Event::AnimationFinished` so one animation
//! can lead to the next.

pub(crate) mod driver;

use std::time::{Duration, Instant};

use rustnative_core::{
    AnimatedProperty, AnimationOwner, AnimationRequest, Event, FrameClock, MotionPreference,
    NodeId, Timeline,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW, WM_APP,
};

use super::runtime::Runtime;

/// Posted to a window by the frame driver when it is time to animate.
pub(crate) const WM_FRAMEWORK_FRAME: u32 = WM_APP + 5;

/// A monotonic clock for real frames.
///
/// Anchored at its own creation rather than at the epoch: the timeline only
/// ever compares times to each other, and an origin close to "now" keeps
/// every duration small.
#[derive(Debug)]
struct RealClock {
    epoch: Instant,
}

impl Default for RealClock {
    fn default() -> Self {
        Self { epoch: Instant::now() }
    }
}

impl FrameClock for RealClock {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }
}

/// One window's animation state.
#[derive(Debug)]
pub(crate) struct AnimationState {
    timeline: Timeline,
    clock: Box<dyn FrameClock>,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self { timeline: Timeline::new(), clock: Box::new(RealClock::default()) }
    }
}

impl AnimationState {
    /// Replaces the clock frames are timed by — how a test animates
    /// without waiting (see `rustnative_core::ManualFrameClock`).
    #[cfg(test)]
    pub(crate) fn set_clock(&mut self, clock: Box<dyn FrameClock>) {
        self.clock = clock;
    }

    /// The window's timeline, for tests that assert on what is running.
    #[cfg(test)]
    pub(crate) const fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Overrides the reduced-motion preference, for tests that must not
    /// depend on how the machine running them is configured.
    #[cfg(test)]
    pub(crate) fn set_motion_preference(&mut self, motion: MotionPreference) {
        self.timeline.set_motion_preference(motion);
    }
}

/// Reads the system's "animate controls and elements inside windows"
/// setting, which is what Windows exposes as the reduced-motion preference.
pub(crate) fn system_motion_preference() -> MotionPreference {
    let mut enabled: i32 = 1;
    // SAFETY: `SPI_GETCLIENTAREAANIMATION` writes one `BOOL` through the
    // pointer; `enabled` is a valid, exclusively borrowed `i32` and the
    // `fWinIni` argument of 0 means "do not broadcast a change".
    let read = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0)
    } != 0;
    // A failure to read the setting is not a reason to take motion away.
    if read && enabled == 0 { MotionPreference::Reduced } else { MotionPreference::Full }
}

/// Applies the system preference to the application and this window's
/// timeline. Called at window creation and on `WM_SETTINGCHANGE`.
pub(crate) fn sync_motion_preference(runtime: &mut Runtime) {
    let preference = system_motion_preference();
    runtime.animation.timeline.set_motion_preference(preference);
    runtime.with_application(|application| application.set_motion_preference(preference));
}

/// Starts the transitions the last render/relayout found, forgets
/// animations of nodes that no longer exist, and starts or stops the frame
/// driver to match.
pub(crate) fn after_render(runtime: &mut Runtime) {
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
    let removed: Vec<NodeId> = runtime.renderer.forgotten_nodes();
    for node in removed {
        runtime.animation.timeline.forget(node);
    }
    sync_driver(runtime);
}

/// Applies the animation requests components made during the dispatch that
/// just finished.
pub(crate) fn apply_requests(runtime: &mut Runtime, requests: Vec<AnimationRequest>) {
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
                cancel_property(runtime, node, property);
            }
            AnimationRequest::CancelOwner(owner) => {
                let frames =
                    runtime.animation.timeline.cancel_owner(AnimationOwner::Component(owner));
                for frame in frames {
                    runtime.renderer.apply_animation_frame(&frame);
                }
            }
            // A request kind this backend has not caught up with: ignoring
            // it leaves the property where the rendered tree puts it.
            _ => {}
        }
    }
    sync_driver(runtime);
}

fn cancel_property(runtime: &mut Runtime, node: NodeId, property: AnimatedProperty) {
    let Some(id) = runtime.animation.timeline.running_id(node, property) else {
        return;
    };
    if let Some(frame) = runtime.animation.timeline.cancel(id) {
        runtime.renderer.apply_animation_frame(&frame);
    }
}

/// One frame: advance the timeline, apply what moved, tell components what
/// ended.
pub(crate) fn frame(runtime: &mut Runtime) {
    // Frame times, for the frame-time budget (`rustnative_core::perf`).
    rustnative_core::perf::frame();
    driver::handled(runtime.window);
    let now = runtime.animation.clock.now();
    let output = runtime.animation.timeline.tick(now);
    for frame in &output.frames {
        runtime.renderer.apply_animation_frame(frame);
    }
    for finished in output.finished {
        // A transition ending is the backend's own business; only an
        // animation a component started is reported back to it.
        if matches!(finished.owner, AnimationOwner::Component(_))
            && !runtime.dispatch_or_quit(Event::AnimationFinished {
                target: finished.node,
                property: finished.property,
            })
        {
            return;
        }
    }
    sync_driver(runtime);
}

/// Starts or stops this window's frames to match whether anything is
/// animating.
pub(crate) fn sync_driver(runtime: &Runtime) {
    if runtime.animation.timeline.is_active() {
        driver::start(runtime.window);
    } else {
        driver::stop(runtime.window);
    }
}

/// Stops this window's frames for good; called as the window is destroyed.
pub(crate) fn release(runtime: &mut Runtime) {
    driver::stop(runtime.window);
}
