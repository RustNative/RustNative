//! The frame signal: one thread that tells animating windows when to
//! produce their next frame.
//!
//! # Why a thread, and why only one
//!
//! Frames must arrive at the display's rhythm, and `DwmFlush` is what
//! Windows offers for that: it blocks until the compositor's next frame.
//! Blocking is exactly what a UI thread must not do, so the wait happens
//! here and the result is a posted message — never a sent one, so the UI
//! thread picks it up on its own terms, with no runtime borrowed.
//!
//! One thread serves every window in the process. It is created the first
//! time something animates and then **sleeps on a condition variable
//! whenever nothing is animating**: an idle application posts no messages
//! and burns no CPU, which is the difference between an animation system
//! and a busy loop.
//!
//! # Coalescing
//!
//! A window that has not yet handled its last frame message is not sent
//! another. If the UI thread falls behind (a long render, a modal loop),
//! frames are dropped rather than queued — the timeline is time-based, so
//! the next frame it *does* handle computes the right value for that
//! moment rather than replaying a backlog.

use std::collections::HashSet;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::WM_FRAMEWORK_FRAME;

/// How long to wait between frames when the compositor cannot pace them
/// (`DwmFlush` failing, which it does when composition is off): 60 Hz.
const FALLBACK_FRAME: Duration = Duration::from_millis(16);

#[derive(Default)]
struct Shared {
    /// Windows that want frames, by handle value.
    animating: HashSet<usize>,
    /// Windows with a frame message already posted and not yet handled.
    pending: HashSet<usize>,
}

/// The process-wide frame driver. See the module documentation.
pub(crate) struct FrameDriver {
    shared: Mutex<Shared>,
    wake: Condvar,
}

impl FrameDriver {
    fn instance() -> &'static Self {
        static DRIVER: OnceLock<FrameDriver> = OnceLock::new();
        DRIVER.get_or_init(|| Self { shared: Mutex::new(Shared::default()), wake: Condvar::new() })
    }

    /// Starts sending frames to `window`.
    fn start(&'static self, window: usize) {
        let mut shared = self.lock();
        if !shared.animating.insert(window) {
            return;
        }
        drop(shared);
        self.ensure_thread();
        self.wake.notify_all();
    }

    /// Stops sending frames to `window`.
    fn stop(&'static self, window: usize) {
        let mut shared = self.lock();
        shared.animating.remove(&window);
        shared.pending.remove(&window);
    }

    /// Records that `window` has handled the frame it was sent, so the next
    /// one may be posted.
    fn handled(&'static self, window: usize) {
        self.lock().pending.remove(&window);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn ensure_thread(&'static self) {
        static STARTED: OnceLock<()> = OnceLock::new();
        STARTED.get_or_init(|| {
            let spawned = std::thread::Builder::new()
                .name("framework-frames".to_owned())
                .spawn(move || self.run());
            // Without the thread, animations simply never advance; every
            // value still reaches its target the moment something else
            // pumps a frame, and nothing else in the application is
            // affected.
            super::super::win32::best_effort(
                spawned.is_ok(),
                "spawn(frame driver)",
                "animations do not advance",
            );
        });
    }

    /// The frame loop: wait for something to animate, pace with the
    /// compositor, post.
    fn run(&'static self) {
        loop {
            let targets = {
                let mut shared = self.lock();
                while shared.animating.is_empty() {
                    // Nothing is animating: sleep until something is. No
                    // timeout, so an idle application costs nothing.
                    shared =
                        self.wake.wait(shared).unwrap_or_else(std::sync::PoisonError::into_inner);
                }
                let targets: Vec<usize> =
                    shared.animating.difference(&shared.pending).copied().collect();
                for window in &targets {
                    shared.pending.insert(*window);
                }
                targets
            };

            for window in targets {
                // SAFETY: `window` is a handle value a UI thread registered
                // while its window was alive. A window destroyed since then
                // makes `PostMessageW` fail, which is why the result is
                // ignored rather than trusted — the window is removed from
                // the set by `stop` on its way out either way.
                let posted = unsafe { PostMessageW(window as HWND, WM_FRAMEWORK_FRAME, 0, 0) };
                if posted == 0 {
                    self.stop(window);
                }
            }

            // Pace with the display. `DwmFlush` returns an error when
            // composition is unavailable, in which case a plain sleep keeps
            // frames coming at a reasonable rate.
            // SAFETY: takes no arguments.
            if unsafe { DwmFlush() } != 0 {
                std::thread::sleep(FALLBACK_FRAME);
            }
        }
    }
}

/// Starts frames for `window`.
pub(crate) fn start(window: HWND) {
    FrameDriver::instance().start(window as usize);
}

/// Stops frames for `window`.
pub(crate) fn stop(window: HWND) {
    FrameDriver::instance().stop(window as usize);
}

/// Whether `window` is currently being sent frames — the observable
/// "is anything still animating?" fact a test can assert on.
#[cfg(test)]
pub(crate) fn is_running(window: HWND) -> bool {
    FrameDriver::instance().lock().animating.contains(&(window as usize))
}

/// Records that `window` handled its frame.
pub(crate) fn handled(window: HWND) {
    FrameDriver::instance().handled(window as usize);
}
