//! The Milestone 41 guarantees on the Linux backend (`docs/guarantees.md`):
//! the shared suites from `rustnative-conformance`, run through the GTK
//! harness.

use std::time::Duration;

use gtk::prelude::*;
use rustnative_conformance::host::{ConformanceHost, Driver};
use rustnative_core::{Application, Component, Window, WindowId};

use super::testing::{Harness, on_gtk, pump_for};

/// The GTK harness as a conformance host.
struct LinuxHost;

struct LinuxDriver<'a> {
    harness: &'a Harness,
}

impl Driver for LinuxDriver<'_> {
    fn click(&mut self, key: &str) {
        self.harness.click(key);
    }

    fn type_text(&mut self, key: &str, text: &str) {
        // One insertion per character at the caret, as keystrokes arrive:
        // GTK's entry inserts it and emits `changed`, as for a keystroke.
        let entry = self.harness.expect_as::<gtk::Entry>(key);
        for character in text.chars() {
            let mut position = entry.position();
            entry.insert_text(&character.to_string(), &mut position);
            entry.set_position(position);
            self.harness.pump();
        }
    }

    fn scroll(&mut self, key: &str, dy: i32) {
        let scrolled = self.harness.expect_as::<gtk::ScrolledWindow>(key);
        let adjustment = scrolled.vadjustment();
        adjustment.set_value(adjustment.value() + f64::from(dy));
        self.harness.pump();
    }

    fn advance(&mut self, duration: Duration) {
        pump_for(duration);
        self.harness.pump();
    }

    fn realized_objects(&self) -> usize {
        self.harness.with_registry(|registry| {
            registry
                .windows
                .get(&WindowId::PRIMARY)
                .map_or(0, |runtime| runtime.renderer.registry.len())
        })
    }
}

impl ConformanceHost for LinuxHost {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn run<C, F>(&mut self, window: Window, root: F, script: &mut dyn FnMut(&mut dyn Driver))
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        let mut application = Application::new(root(), window);
        // SAFETY: `application` is declared before the harness and outlives
        // it.
        let harness = unsafe { Harness::attach(&mut application) };
        script(&mut LinuxDriver { harness: &harness });
    }
}

#[test]
fn the_shared_guarantee_suites_hold_on_linux() {
    on_gtk(|| rustnative_conformance::suites::all(&mut LinuxHost));
}
