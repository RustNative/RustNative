//! Telling a component that one of its virtual lists needs a different
//! window of items.
//!
//! The renderer works out *that* a list's visible range changed (see
//! `rendering::virtual_list`); this is the one place that turns that into
//! an [`Event::VisibleRangeChanged`] and hands it to the component — the
//! same split the animation subsystem uses, and for the same reason: a
//! dispatch renders, and a renderer must not call back into the thing
//! rendering it.
//!
//! # Why this is a loop rather than a call
//!
//! Answering a range change renders new items, which lays out, which can
//! compute a further range — an item that measured taller than estimated
//! genuinely changes how many fit. That inner pass queues its change
//! instead of dispatching it (`VirtualLists::is_dispatching`), and the loop
//! here picks it up. The loop terminates because each pass either changes
//! no range (done) or realizes the range the previous one asked for, and
//! measurements do not depend on where an item was placed; the cap is a
//! backstop against an application that renders a different item count than
//! it was asked for, not the expected exit.

use rustnative_core::Event;

use super::runtime::Runtime;

/// How many times one render may re-enter the range loop before the
/// backstop stops it. Two passes is the normal maximum (a range change,
/// then the measurement it settles at).
const MAX_PASSES: usize = 8;

/// Reports every virtual list whose visible range changed, rendering the
/// items each component returns in answer.
pub(crate) fn after_render(runtime: &mut Runtime) {
    if runtime.renderer.virtual_lists.is_dispatching() {
        // A nested pass: the outer loop below is still running and will
        // pick up whatever this one queued.
        return;
    }
    runtime.renderer.virtual_lists.set_dispatching(true);
    for _ in 0..MAX_PASSES {
        let changes = runtime.renderer.take_range_changes();
        if changes.is_empty() {
            break;
        }
        for (target, range) in changes {
            if !runtime.dispatch_or_quit(Event::VisibleRangeChanged { target, range }) {
                runtime.renderer.virtual_lists.set_dispatching(false);
                return;
            }
        }
    }
    runtime.renderer.virtual_lists.set_dispatching(false);
}
