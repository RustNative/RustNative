//! The terminal backend's budget scenarios (`PLAN.md` Milestone 42,
//! `budgets/tui.toml`), on the synthetic terminal — the same session loop
//! the platform runs, writing real escape sequences into a VT emulator:
//!
//! - `launch_ms`: launch to the first frame on a 120 × 40 terminal;
//! - `input_latency_ms`: Enter on a focused button to the frame showing it;
//! - `bytes_per_keystroke`: what typing into a field writes, per key;
//! - `frame_time_full_ms` / `frame_time_full_max_ms`: a frame in which every
//!   row of the screen changes;
//! - `resident_memory_mb`: the process with the screens alive.
//!
//! It prints one JSON object, as `examples/bench-app` does.

#![allow(
    clippy::expect_used,
    reason = "a measurement harness: a step that fails is a failed run, reported by its exit status"
)]

use std::time::{Duration, Instant};

use rustnative_core::{
    ColumnStyle, Component, EdgeInsets, Event, KeyCode, KeyModifiers, LayoutStyle, Node, NodeId,
    Size, Window,
};
use rustnative_tui::TuiHarness;
use serde_json::json;

const COLUMNS: u16 = 120;
const ROWS: u16 = 40;

/// Forty rows of text, a button, and a field.
struct Screen {
    clicks: u32,
    text: String,
    generation: u32,
}

impl Component for Screen {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { clicks: 0, text: String::new(), generation: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let mut children = vec![
            Node::label("clicks", format!("clicks {}", self.clicks)),
            Node::button("increment", "Increment"),
            Node::text_input("field", self.text.clone()),
        ];
        children.extend((0..36).map(|row| {
            Node::label(
                format!("row-{row}"),
                format!(
                    "row {row:>2} generation {:>6} — {}",
                    self.generation,
                    "the quick brown fox jumps over the lazy dog ".repeat(2)
                ),
            )
        }));
        Node::column_with_layout(
            "root",
            children,
            LayoutStyle::new(),
            ColumnStyle::new().padding(EdgeInsets::all(0)).gap(0),
        )
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("increment") => self.clicks += 1,
            Event::TextChanged { value, .. } => self.text = value,
            Event::KeyDown { key: KeyCode::Function(5), .. } => self.generation += 1,
            _ => {}
        }
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples.get(samples.len() / 2).copied().unwrap_or_default()
}

fn launch() -> TuiHarness {
    TuiHarness::launch(Window::new("tui-bench", Size::new(0, 0)), COLUMNS, ROWS, || Screen::new(()))
}

#[cfg(target_os = "linux")]
fn resident_memory_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse::<f64>().ok())
        .map_or(f64::NAN, |kilobytes| kilobytes / 1024.0)
}

#[cfg(windows)]
fn resident_memory_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).unwrap_or(0),
        ..PROCESS_MEMORY_COUNTERS::default()
    };
    // SAFETY: this process's pseudo-handle; `counters` is sized as `cb` says.
    let read =
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, counters.cb) };
    #[allow(clippy::cast_precision_loss, reason = "megabytes need no more precision")]
    let megabytes = counters.WorkingSetSize as f64 / (1024.0 * 1024.0);
    if read == 0 { f64::NAN } else { megabytes }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn resident_memory_mb() -> f64 {
    f64::NAN
}

fn main() {
    let launches: Vec<Duration> = (0..11)
        .map(|_| {
            let started = Instant::now();
            let terminal = launch();
            let elapsed = started.elapsed();
            assert!(terminal.stats().frames >= 1, "a first frame was written");
            elapsed
        })
        .collect();

    let mut terminal = launch();
    terminal.press_tab();
    let latencies: Vec<Duration> = (0..31)
        .map(|_| {
            let started = Instant::now();
            terminal.press_enter();
            started.elapsed()
        })
        .collect();
    assert!(terminal.text().contains("clicks 31"));

    terminal.press_tab();
    let _ = terminal.take_written();
    let typed = "the quick brown fox jumps over the lazy dog";
    terminal.type_text(typed);
    #[allow(clippy::cast_precision_loss, reason = "a byte count")]
    let per_key = terminal.take_written().len() as f64 / typed.chars().count() as f64;

    let frames: Vec<Duration> = (0..30)
        .map(|_| {
            let started = Instant::now();
            terminal.press(KeyCode::Function(5), KeyModifiers::default());
            started.elapsed()
        })
        .collect();
    let worst = frames.iter().max().copied().unwrap_or_default();

    let measured = json!({
        "launch_ms": millis(median(launches)),
        "input_latency_ms": millis(median(latencies)),
        "bytes_per_keystroke": per_key,
        "frame_time_full_ms": millis(median(frames)),
        "frame_time_full_max_ms": millis(worst),
        "resident_memory_mb": resident_memory_mb(),
    });
    println!("{measured}");
}
