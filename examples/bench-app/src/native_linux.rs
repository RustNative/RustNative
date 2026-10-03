//! The native scenarios on Linux (`PLAN.md` Milestone 42): the same
//! measurements as on Windows, through GTK. The driver runs on another
//! thread and reaches the widgets the way anything outside GTK's thread
//! must — by invoking work on its main context.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use rustnative_core::perf::{self, StartupPhase};
use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_linux::LinuxPlatform;
use serde_json::{Value, json};

use super::{TITLE, millis};

fn wait_for(phase: StartupPhase) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !perf::reached(phase) {
        assert!(Instant::now() < deadline, "the application never became {}", phase.name());
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Runs `work` on the GTK thread and waits for it.
fn on_gtk<R: Send + 'static>(work: impl FnOnce() -> R + Send + 'static) -> R {
    let (reply, answer) = std::sync::mpsc::channel();
    gtk::glib::MainContext::default().invoke(move || {
        let _ = reply.send(work());
    });
    answer.recv().expect("the GTK thread answers")
}

fn close() {
    on_gtk(|| {
        for window in gtk::Window::list_toplevels() {
            if let Ok(window) = window.downcast::<gtk::Window>() {
                window.close();
            }
        }
    });
}

/// The resident set, from `/proc/self/status`.
fn resident_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kilobytes = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
        .unwrap_or(f64::NAN);
    kilobytes / 1024.0
}

/// Runs `root` on the Linux backend while `driver` runs on another thread;
/// `driver` closes the window when it is done.
fn run<C: Component>(root: C, driver: impl FnOnce() -> Value + Send + 'static) -> Value {
    let driver = std::thread::spawn(driver);
    let mut application = Application::new(root, Window::new(TITLE, Size::new(480, 720)));
    let ran = LinuxPlatform::new().run(&mut application);
    assert!(ran.is_ok(), "the backend failed: {ran:?}");
    driver.join().unwrap_or_else(|_| json!({ "error": "the driver panicked" }))
}

pub(super) fn startup() -> Value {
    run(super::Screen::new(()), || {
        wait_for(StartupPhase::Interactive);
        let resident = resident_mb();
        close();
        let trace = perf::startup_trace();
        let at = |phase| trace.get(phase).map_or(f64::NAN, millis);
        json!({
            "runtime_ready_ms": at(StartupPhase::RuntimeReady),
            "first_frame_ms": at(StartupPhase::FirstFrame),
            "first_content_ms": at(StartupPhase::FirstContent),
            "interactive_ms": at(StartupPhase::Interactive),
            "cold_start_ms": at(StartupPhase::Interactive),
            "resident_memory_mb": resident,
        })
    })
}

/// The first descendant of any window that is a `T` and satisfies `matches`.
fn find<T: IsA<gtk::Widget>>(matches: &dyn Fn(&T) -> bool) -> Option<T> {
    fn walk<T: IsA<gtk::Widget>>(widget: &gtk::Widget, matches: &dyn Fn(&T) -> bool) -> Option<T> {
        if let Some(found) = widget.downcast_ref::<T>().filter(|candidate| matches(candidate)) {
            return Some(found.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = walk(&current, matches) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    gtk::Window::list_toplevels().iter().find_map(|window| walk(window, matches))
}

thread_local! {
    /// The widget a scenario drives, found once (on the GTK thread).
    static TARGET: RefCell<Option<gtk::Widget>> = const { RefCell::new(None) };
}

/// Times `count` inputs, each delivered by `input` on the GTK thread, until
/// the change it caused is realized.
fn time_inputs(
    count: usize,
    pause: Duration,
    input: impl Fn(usize) + Send + Sync + Clone + 'static,
) -> Vec<Duration> {
    let mut latencies = Vec::new();
    for index in 0..count {
        let started = Instant::now();
        let input = input.clone();
        gtk::glib::MainContext::default().invoke(move || input(index));
        let deadline = started + Duration::from_secs(5);
        while perf::last_realized().is_none_or(|at| at <= started) {
            assert!(Instant::now() < deadline, "the input was never realized");
            std::hint::spin_loop();
        }
        latencies.push(perf::last_realized().unwrap_or(started).saturating_duration_since(started));
        std::thread::sleep(pause);
    }
    latencies
}

pub(super) fn interaction() -> Value {
    run(super::Screen::new(()), || {
        wait_for(StartupPhase::Interactive);
        let found = on_gtk(|| {
            let button =
                find::<gtk::Button>(&|button| button.label().as_deref() == Some("Increment"));
            let found = button.is_some();
            TARGET.with(|target| *target.borrow_mut() = button.map(Cast::upcast));
            found
        });
        assert!(found, "the bench screen's button");
        let latencies = time_inputs(31, Duration::from_millis(20), |_| {
            TARGET.with(|target| {
                if let Some(button) =
                    target.borrow().as_ref().and_then(|widget| widget.downcast_ref::<gtk::Button>())
                {
                    button.emit_clicked();
                }
            });
        });
        close();
        json!({
            "input_latency_ms": perf::percentile(&latencies, 50).map_or(f64::NAN, millis),
            "input_latency_max_ms": perf::percentile(&latencies, 100).map_or(f64::NAN, millis),
        })
    })
}

/// Milestone 54: keystrokes typed into the real entry of the filter over
/// 200 000 rows, each timed until its echo is realized, while the filtered
/// view updates behind them; then the time until the last filter's result
/// is shown.
pub(super) fn filter() -> Value {
    run(filter_demo::App::new(()), || {
        wait_for(StartupPhase::Interactive);
        let found = on_gtk(|| {
            let entry = find::<gtk::Entry>(&|_| true);
            let found = entry.is_some();
            TARGET.with(|target| *target.borrow_mut() = entry.map(Cast::upcast));
            found
        });
        assert!(found, "the query field");
        let text: Vec<char> = "amber falcon".chars().collect();
        let latencies = time_inputs(text.len(), Duration::from_millis(30), move |index| {
            TARGET.with(|target| {
                if let Some(entry) =
                    target.borrow().as_ref().and_then(|widget| widget.downcast_ref::<gtk::Entry>())
                {
                    let mut position = -1;
                    entry.insert_text(&text[index].to_string(), &mut position);
                }
            });
        });
        let typed = Instant::now();
        let deadline = typed + Duration::from_secs(10);
        while on_gtk(|| {
            find::<gtk::Label>(&|label| label.text().contains("rows match"))
                .is_some_and(|status| status.text().contains("updating"))
        }) {
            assert!(Instant::now() < deadline, "the filter never finished");
            std::thread::sleep(Duration::from_millis(1));
        }
        let settled = typed.elapsed();
        close();
        json!({
            "filter_input_latency_ms": perf::percentile(&latencies, 50).map_or(f64::NAN, millis),
            "filter_input_latency_max_ms": perf::percentile(&latencies, 100).map_or(f64::NAN, millis),
            "filter_results_ms": millis(settled),
        })
    })
}

pub(super) fn animation() -> Value {
    run(super::Animated::new(()), || {
        wait_for(StartupPhase::Interactive);
        perf::record_frames();
        std::thread::sleep(Duration::from_millis(3_200));
        let frames = perf::take_frame_times();
        close();
        let at = |p| perf::percentile(&frames, p).map_or(f64::NAN, millis);
        json!({
            "frames": frames.len(),
            "frame_time_p50_ms": at(50),
            "frame_time_p99_ms": at(99),
            "frame_time_max_ms": at(100),
        })
    })
}
