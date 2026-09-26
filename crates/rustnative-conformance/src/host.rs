//! The seam the guarantee suites run through (`PLAN.md` Milestone 41): one
//! suite, every backend. A backend implements [`ConformanceHost`] — the
//! headless backend here ([`HeadlessHost`]), Windows in its own test module
//! over its native harness — and each suite in [`crate::suites`] runs
//! unchanged on each.

use std::time::Duration;

use rustnative_core::{Component, Window};
use rustnative_headless::{HeadlessApp, Query};

/// What a suite does to a running application, the way a person would.
pub trait Driver {
    /// Clicks the control whose key is `key`.
    fn click(&mut self, key: &str);
    /// Types `text` into the field whose key is `key`, one keystroke at a
    /// time.
    fn type_text(&mut self, key: &str, text: &str);
    /// Scrolls the container whose key is `key` by `dy` pixels.
    fn scroll(&mut self, key: &str, dy: i32);
    /// Lets `duration` pass, running whatever comes due.
    fn advance(&mut self, duration: Duration);
    /// How many native objects the backend has realized for the
    /// application's primary window right now.
    fn realized_objects(&self) -> usize;
}

/// A backend the suites can run on.
pub trait ConformanceHost {
    /// Its name, for failure messages.
    fn name(&self) -> &'static str;

    /// Launches `root` in `window`, runs `script` against it, and tears it
    /// down.
    fn run<C, F>(&mut self, window: Window, root: F, script: &mut dyn FnMut(&mut dyn Driver))
    where
        C: Component,
        F: Fn() -> C + 'static;
}

/// The headless reference backend (Milestone 45) as a conformance host.
#[derive(Debug, Default)]
pub struct HeadlessHost;

struct HeadlessDriver(HeadlessApp);

impl Driver for HeadlessDriver {
    fn click(&mut self, key: &str) {
        if let Err(error) = self.0.click(&Query::key(key)) {
            panic!("click `{key}`: {error}");
        }
    }

    fn type_text(&mut self, key: &str, text: &str) {
        if let Err(error) = self.0.type_text(&Query::key(key), text) {
            panic!("type into `{key}`: {error}");
        }
    }

    fn scroll(&mut self, key: &str, dy: i32) {
        if let Err(error) = self.0.scroll(&Query::key(key), dy) {
            panic!("scroll `{key}`: {error}");
        }
    }

    fn advance(&mut self, duration: Duration) {
        self.0.advance(duration);
    }

    fn realized_objects(&self) -> usize {
        self.0.realized().nodes().count()
    }
}

impl ConformanceHost for HeadlessHost {
    fn name(&self) -> &'static str {
        "headless"
    }

    fn run<C, F>(&mut self, window: Window, root: F, script: &mut dyn FnMut(&mut dyn Driver))
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        let mut driver = HeadlessDriver(HeadlessApp::launch(window, root));
        script(&mut driver);
    }
}
