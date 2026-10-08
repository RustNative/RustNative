//! Driving `rustnative_core`'s `Timeline` from `Choreographer`, and
//! applying what it produces to views.
//!
//! ```text
//! render / relayout   the renderer notices a transitioned property changed
//!        │            and records what it moved from and to
//!        ▼
//! after_render        those become timeline transitions; a frame callback
//!        │            is requested if anything now animates
//!        ▼
//! frame               Choreographer, once per display frame: advance the
//!        │            timeline, apply each value, ask for the next frame
//!        ▼
//! idle                nothing animating: no frame is asked for
//! ```
//!
//! No step touches a component except an animation a component started
//! ending, which arrives as `Event::AnimationFinished`. Reduced motion
//! follows the system's animator duration scale (`host_traits`).

use std::time::{Duration, Instant};

use rustnative_core::{AnimationOwner, AnimationRequest, Event, FrameClock, Timeline, WindowId};

use crate::Error;
use crate::jni_host::{Arg, Class, call_static};
use crate::registry::WindowRegistry;

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
    /// Whether a frame callback is on its way.
    frame_requested: bool,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self {
            timeline: Timeline::new(),
            clock: Box::new(RealClock { epoch: Instant::now() }),
            frame_requested: false,
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

/// Starts the transitions the last render or layout found, forgets
/// animations of removed nodes, and asks for frames while anything moves.
pub(crate) fn after_render(registry: &mut WindowRegistry, window: WindowId) -> Result<(), Error> {
    let motion = registry.traits.motion;
    let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
    let Some(renderer) = runtime.renderer.as_mut() else { return Ok(()) };
    runtime.animation.timeline.set_motion_preference(motion);
    let now = runtime.animation.clock.now();
    for request in renderer.take_transitions() {
        runtime.animation.timeline.transition(
            request.node,
            request.property,
            request.from,
            request.to,
            request.transition,
            now,
        );
    }
    for node in renderer.forgotten_nodes() {
        runtime.animation.timeline.forget(node);
    }
    sync_frames(registry, window)
}

/// Applies the animation requests components made during the dispatch
/// that just finished.
pub(crate) fn apply_requests(registry: &mut WindowRegistry, window: WindowId) -> Result<(), Error> {
    let requests =
        registry.with_application(|application| application.take_animation_requests(window));
    if requests.is_empty() {
        return Ok(());
    }
    let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
    let Some(renderer) = runtime.renderer.as_mut() else { return Ok(()) };
    let now = runtime.animation.clock.now();
    for request in requests {
        match request {
            AnimationRequest::Start { node, animation, owner } => {
                if !renderer.snapshot.contains(node) {
                    continue;
                }
                let current = renderer.current_value(node, animation.property());
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
                        renderer.apply_animation_frame(&frame)?;
                    }
                }
            }
            AnimationRequest::CancelOwner(owner) => {
                for frame in
                    runtime.animation.timeline.cancel_owner(AnimationOwner::Component(owner))
                {
                    renderer.apply_animation_frame(&frame)?;
                }
            }
            _ => {}
        }
    }
    sync_frames(registry, window)
}

/// One display frame: advance the timeline, apply what moved, tell
/// components what ended.
pub(crate) fn frame(
    registry: &mut WindowRegistry,
    window: WindowId,
    _nanos: i64,
) -> Result<(), Error> {
    rustnative_core::perf::frame();
    let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
    runtime.animation.frame_requested = false;
    let now = runtime.animation.clock.now();
    let output = runtime.animation.timeline.tick(now);
    if let Some(renderer) = runtime.renderer.as_mut() {
        for frame in &output.frames {
            renderer.apply_animation_frame(frame)?;
        }
    }
    for finished in output.finished {
        if matches!(finished.owner, AnimationOwner::Component(_)) {
            registry.dispatch(
                window,
                Event::AnimationFinished { target: finished.node, property: finished.property },
            )?;
        }
    }
    sync_frames(registry, window)
}

/// Asks for the next frame while anything animates.
fn sync_frames(registry: &mut WindowRegistry, window: WindowId) -> Result<(), Error> {
    let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
    let active =
        runtime.animation.timeline.is_active() && !runtime.destroyed && runtime.activity.is_some();
    if active && !runtime.animation.frame_requested {
        runtime.animation.frame_requested = true;
        call_static(
            Class::Bridge,
            "requestFrame",
            "(J)V",
            &[Arg::Long(i64::try_from(window.get()).unwrap_or(0))],
        )?;
    }
    Ok(())
}
