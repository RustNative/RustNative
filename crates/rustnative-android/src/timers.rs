//! Timers on the main looper (`RnBridge.schedule` → `nativeTimer`): what
//! the backend waits for, such as a gesture's long-press deadline.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use rustnative_core::WindowId;

use crate::Error;
use crate::jni_host::{Arg, Class, call_static};
use crate::registry::WindowRegistry;

/// What a timer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Timer {
    /// A gesture recognizer's long-press deadline in a window.
    LongPress(WindowId),
}

thread_local! {
    static TIMERS: RefCell<(i64, HashMap<i64, Timer>)> = RefCell::new((0, HashMap::new()));
}

/// Schedules `timer` after `wait`; returns its token.
pub(crate) fn schedule(timer: Timer, wait: Duration) -> Result<i64, Error> {
    let token = TIMERS.with(|timers| {
        let mut timers = timers.borrow_mut();
        timers.0 += 1;
        let token = timers.0;
        timers.1.insert(token, timer);
        token
    });
    let millis = i64::try_from(wait.as_millis()).unwrap_or(i64::MAX);
    call_static(Class::Bridge, "schedule", "(JJ)V", &[Arg::Long(token), Arg::Long(millis)])?;
    Ok(token)
}

/// Timer `token` fired.
pub(crate) fn fired(registry: &mut WindowRegistry, token: i64) -> Result<(), Error> {
    let timer = TIMERS.with(|timers| timers.borrow_mut().1.remove(&token));
    match timer {
        Some(Timer::LongPress(window)) => {
            crate::input::pointer::long_press(registry, window, token)
        }
        None => crate::services::timer_fired(registry, token),
    }
}
