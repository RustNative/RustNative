//! The budget scenarios (`PLAN.md` Milestone 42): what `rustnative bench`
//! runs and measures against `budgets/<target>.toml`.
//!
//! Each run performs one scenario and prints one JSON object of
//! measurements on standard output:
//!
//! - `startup` (Windows): the startup phases from the process's own
//!   creation time, and the resident memory once interactive;
//! - `interaction` (Windows): synthetic clicks posted to the real button
//!   window, each timed until the change it causes is realized;
//! - `filter` (Windows): keystrokes posted to the filter over 200 000 rows
//!   (`examples/filter-demo`), each timed until its echo is realized while
//!   the filtered view updates, then the time until the last result shows;
//! - `animation` (Windows): a size transition run repeatedly, its frame
//!   intervals recorded;
//! - `compile` (any machine): the markup and style compile steps;
//! - `core` (any machine): diffing, laying out, and re-rendering a tree of
//!   a thousand nodes;
//! - `headless` (any machine): launching the bench screen on the headless
//!   backend, a click's latency through its input path, and a keystroke's
//!   handling in the filter over 200 000 rows.
//!
//! `--low-end` pins the process to one core first: the low-end reference
//! profile, until a device matrix exists.

#![allow(
    clippy::expect_used,
    reason = "a measurement harness: a step that fails is a failed run, reported by its exit status"
)]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use rustnative_core::perf;
use rustnative_core::{
    AnimatedProperty, Component, ComponentContext, ComponentTree, Event, LayoutEngine, LayoutStyle,
    Node, NodeId, Size, SizeMode, Transition, TreeDiff, TreeSnapshot, Window,
};
use serde_json::{Value, json};

/// The window title the helper threads find the window by.
const TITLE: &str = "RustNative budget scenario";

/// A typical form: a heading, forty labelled rows, and a counter.
struct Screen {
    clicks: u32,
}

impl Component for Screen {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { clicks: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let rows = (0..40).map(|row| {
            Node::row(
                format!("row-{row}"),
                [
                    Node::label(format!("label-{row}"), format!("Field {row}")),
                    Node::text_input(format!("input-{row}"), ""),
                ],
            )
        });
        Node::column(
            "screen",
            [
                Node::label("heading", "Budget scenario"),
                Node::label("count", self.clicks.to_string()),
            ]
            .into_iter()
            .chain([Node::button("increment", "Increment")])
            .chain(rows),
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.clicks += 1;
        }
    }
}

/// A box whose width transitions each time a timer flips it.
struct Animated {
    wide: bool,
}

impl Component for Animated {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { wide: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let width = if self.wide { 400 } else { 40 };
        Node::column(
            "stage",
            [Node::label_with_layout(
                "box",
                "moving",
                LayoutStyle::new().width(SizeMode::Fixed(width)),
            )
            .with_transition(AnimatedProperty::Size, Transition::new(Duration::from_millis(900)))],
        )
    }
    fn update(&mut self, _: Event) {}
    fn message(&mut self, (): ()) {
        self.wide = !self.wide;
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let delay = context.sleep(Duration::from_millis(1_000));
        context.spawn(delay);
        self.view()
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e6
}

/// The median of `f` over `runs` runs.
fn median(runs: usize, mut f: impl FnMut() -> Duration) -> Duration {
    let samples: Vec<Duration> = (0..runs).map(|_| f()).collect();
    perf::percentile(&samples, 50).unwrap_or_default()
}

fn thousand_labels(changed: usize) -> Node {
    Node::column(
        "list",
        (0..1_000).map(|index| {
            let text =
                if index < changed { format!("changed {index}") } else { format!("row {index}") };
            Node::label(format!("row-{index}"), text)
        }),
    )
}

/// A component rendering a thousand rows, for the re-render cost.
struct List {
    generation: u32,
}

impl Component for List {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { generation: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "list",
            [Node::button("bump", "Bump")].into_iter().chain((0..1_000).map(|index| {
                Node::label(format!("row-{index}"), format!("{} {index}", self.generation))
            })),
        )
    }
    fn update(&mut self, _: Event) {
        self.generation += 1;
    }
}

fn core() -> Value {
    let before = TreeSnapshot::from_node(&thousand_labels(0)).unwrap_or_default();
    let after = TreeSnapshot::from_node(&thousand_labels(10)).unwrap_or_default();
    let diff = median(31, || {
        let started = Instant::now();
        let diff = TreeDiff::between(&before, &after);
        let elapsed = started.elapsed();
        assert_eq!(diff.operations().len(), 10);
        elapsed
    });
    let layout = median(31, || {
        let started = Instant::now();
        let rects = LayoutEngine::new().layout(&after, Size::new(800, 100_000));
        let elapsed = started.elapsed();
        assert_eq!(rects.len(), 1_001);
        elapsed
    });
    let mut tree = ComponentTree::new(List::new(()));
    let render = median(31, || {
        let started = Instant::now();
        tree.dispatch(Event::Click { target: NodeId::from_key("bump") });
        started.elapsed()
    });
    json!({
        "tree_diff_us_1k_nodes": micros(diff),
        "layout_us_1k_nodes": micros(layout),
        "render_us_1k_nodes": micros(render),
    })
}

/// The markup and style compile steps: lowering about a thousand lines of
/// `.rsx`, and resolving a typical class string — the same parser and
/// vocabulary the build runs.
fn compile() -> Value {
    use std::fmt::Write as _;
    let mut source = String::from(
        "fn view() -> Node {
    <Column key=\"root\">
",
    );
    for index in 0..125 {
        let _ = writeln!(source, "        <Row key=\"row-{index}\">");
        let _ = writeln!(source, "            <Label key=\"label-{index}\"");
        let _ = writeln!(source, "                text=\"Field {index}\" />");
        let _ = writeln!(source, "            <TextInput key=\"input-{index}\"");
        let _ = writeln!(source, "                value=\"\" />");
        let _ = writeln!(source, "            <Button key=\"go-{index}\" text=\"Go\" />");
        let _ = writeln!(source, "        </Row>");
    }
    source.push_str(
        "    </Column>
}
",
    );
    let options = rustnative_markup::CompileOptions {
        source_path: "bench.rsx".into(),
        src_root: ".".into(),
        wrapper: None,
    };
    let compile = median(9, || {
        let started = Instant::now();
        let compiled = rustnative_markup::compile(&source, &options);
        let elapsed = started.elapsed();
        assert!(compiled.is_ok(), "the bench markup compiles");
        elapsed
    });
    #[allow(clippy::cast_precision_loss, reason = "a line count")]
    let lines = source.lines().count() as f64;
    let vocabulary = rustnative_style::Vocabulary::defaults();
    let classes =
        "flex items-center gap-2 px-4 py-2 rounded-lg bg-blue-500 hover:bg-blue-600 text-white";
    let resolve = median(101, || {
        let started = Instant::now();
        let resolved = vocabulary.resolve_classes(classes);
        let elapsed = started.elapsed();
        assert!(resolved.is_ok());
        elapsed
    });
    json!({
        "rsx_compile_ms_per_kloc": millis(compile) * 1_000.0 / lines,
        "class_resolve_us": micros(resolve),
    })
}

fn headless() -> Value {
    use rustnative_headless::{HeadlessApp, Query};
    let launch = median(11, || {
        let started = Instant::now();
        let app = HeadlessApp::launch(Window::new(TITLE, Size::new(480, 720)), || Screen::new(()));
        let elapsed = started.elapsed();
        drop(app);
        elapsed
    });
    let mut app = HeadlessApp::launch(Window::new(TITLE, Size::new(480, 720)), || Screen::new(()));
    let click = Query::key("increment");
    let latency = median(31, || {
        let started = Instant::now();
        let clicked = app.click(&click);
        let elapsed = started.elapsed();
        assert!(clicked.is_ok());
        elapsed
    });
    // Milestone 54: a keystroke into the filter over 200 000 rows, handled
    // (the event and the render it causes) without waiting for the filter.
    let mut filter =
        HeadlessApp::launch(Window::new(TITLE, Size::new(480, 720)), || filter_demo::App::new(()));
    let mut typed = String::new();
    let mut keystrokes = Vec::new();
    for character in "amber falcon".chars() {
        typed.push(character);
        let started = Instant::now();
        filter.application_mut().dispatch(rustnative_core::Event::TextChanged {
            target: rustnative_core::NodeId::from_key("query"),
            value: typed.clone(),
        });
        keystrokes.push(started.elapsed());
    }
    filter.settle();
    let keystroke = rustnative_core::perf::percentile(&keystrokes, 50).unwrap_or_default();
    json!({
        "launch_ms": millis(launch),
        "input_latency_ms": millis(latency),
        "filter_input_latency_ms": millis(keystroke),
    })
}

#[cfg(windows)]
mod native {
    use std::time::{Duration, Instant};

    use rustnative_core::perf::{self, StartupPhase};
    use rustnative_core::{Application, Component, Platform, Size, Window, WindowId};
    use rustnative_windows::WindowsPlatform;
    use serde_json::{Value, json};
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BM_CLICK, EnumChildWindows, GetClassNameW, GetWindowTextW, PostMessageW, WM_CHAR, WM_CLOSE,
    };

    use super::{TITLE, millis};

    fn wait_for(phase: StartupPhase) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !perf::reached(phase) {
            assert!(Instant::now() < deadline, "the application never became {}", phase.name());
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn close() {
        if let Some(hwnd) = rustnative_windows::native_window_handle(WindowId::PRIMARY) {
            // SAFETY: posting to a window of this process; a stale handle
            // makes the post fail harmlessly.
            unsafe { PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0) };
        }
    }

    fn resident_mb() -> f64 {
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).unwrap_or(0),
            ..PROCESS_MEMORY_COUNTERS::default()
        };
        // SAFETY: the pseudo-handle of this process; `counters` is sized
        // as `cb` says.
        let read =
            unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, counters.cb) };
        assert!(read != 0, "K32GetProcessMemoryInfo failed");
        #[allow(clippy::cast_precision_loss, reason = "megabytes need no more precision")]
        let megabytes = counters.WorkingSetSize as f64 / (1024.0 * 1024.0);
        megabytes
    }

    /// Runs `root` on the Windows backend while `driver` runs on another
    /// thread; `driver` closes the window when it is done.
    fn run<C: Component>(root: C, driver: impl FnOnce() -> Value + Send + 'static) -> Value {
        let driver = std::thread::spawn(driver);
        let mut application = Application::new(root, Window::new(TITLE, Size::new(480, 720)));
        let ran = WindowsPlatform::new().run(&mut application);
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

    /// The first descendant of `parent` that is a button captioned
    /// `caption`.
    fn find_button(parent: HWND, caption: &str) -> Option<HWND> {
        struct Search {
            caption: Vec<u16>,
            found: Option<HWND>,
        }
        unsafe extern "system" fn visit(hwnd: HWND, search: LPARAM) -> i32 {
            // SAFETY: `search` is the `Search` passed to `EnumChildWindows`
            // below, alive for the whole enumeration.
            let search = unsafe { &mut *(search as *mut Search) };
            let mut class = [0u16; 32];
            let mut text = [0u16; 64];
            // SAFETY: `hwnd` is the live window being enumerated; each
            // buffer's length is passed.
            let (class_len, text_len) = unsafe {
                (
                    GetClassNameW(hwnd, class.as_mut_ptr(), 32),
                    GetWindowTextW(hwnd, text.as_mut_ptr(), 64),
                )
            };
            let class = String::from_utf16_lossy(&class[..usize::try_from(class_len).unwrap_or(0)]);
            let text = &text[..usize::try_from(text_len).unwrap_or(0)];
            if class.eq_ignore_ascii_case("button") && text == search.caption.as_slice() {
                search.found = Some(hwnd);
                return 0;
            }
            1
        }
        let mut search = Search { caption: caption.encode_utf16().collect(), found: None };
        // SAFETY: `parent` is a live window; `visit` only reads the
        // `Search` it is given, which outlives the call.
        unsafe { EnumChildWindows(parent, Some(visit), (&raw mut search) as LPARAM) };
        search.found
    }

    pub(super) fn interaction() -> Value {
        run(super::Screen::new(()), || {
            wait_for(StartupPhase::Interactive);
            let window =
                rustnative_windows::native_window_handle(WindowId::PRIMARY).unwrap_or(0) as HWND;
            let button = find_button(window, "Increment").expect("the bench screen's button");
            let mut latencies = Vec::new();
            for _ in 0..31 {
                let started = Instant::now();
                // SAFETY: a live button of this process.
                unsafe { PostMessageW(button, BM_CLICK, 0, 0) };
                let deadline = started + Duration::from_secs(5);
                while perf::last_realized().is_none_or(|at| at <= started) {
                    assert!(Instant::now() < deadline, "the click was never realized");
                    std::hint::spin_loop();
                }
                latencies.push(
                    perf::last_realized().unwrap_or(started).saturating_duration_since(started),
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            close();
            json!({
                "input_latency_ms": perf::percentile(&latencies, 50).map_or(f64::NAN, millis),
                "input_latency_max_ms": perf::percentile(&latencies, 100).map_or(f64::NAN, millis),
            })
        })
    }

    /// Milestone 54: keystrokes posted to the real edit control of the
    /// filter over 200 000 rows, each timed until its echo is realized,
    /// while the filtered view updates behind them; then the time until the
    /// last filter's result is shown.
    pub(super) fn filter() -> Value {
        run(filter_demo::App::new(()), || {
            wait_for(StartupPhase::Interactive);
            let window =
                rustnative_windows::native_window_handle(WindowId::PRIMARY).unwrap_or(0) as HWND;
            let edit = find_child(window, "edit", None).expect("the query field");
            let mut latencies = Vec::new();
            for character in "amber falcon".encode_utf16() {
                let started = Instant::now();
                // SAFETY: a live edit control of this process.
                unsafe { PostMessageW(edit, WM_CHAR, usize::from(character), 0) };
                let deadline = started + Duration::from_secs(5);
                while perf::last_realized().is_none_or(|at| at <= started) {
                    assert!(Instant::now() < deadline, "the keystroke was never realized");
                    std::hint::spin_loop();
                }
                latencies.push(
                    perf::last_realized().unwrap_or(started).saturating_duration_since(started),
                );
                std::thread::sleep(Duration::from_millis(30));
            }
            let typed = Instant::now();
            let deadline = typed + Duration::from_secs(10);
            while find_child(window, "static", Some("rows match"))
                .is_some_and(|status| text_of(status).contains("updating"))
            {
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

    fn text_of(hwnd: HWND) -> String {
        let mut text = [0u16; 128];
        // SAFETY: a live window; the buffer's length is passed.
        let length = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), 128) };
        String::from_utf16_lossy(&text[..usize::try_from(length).unwrap_or(0)])
    }

    /// The first descendant of `parent` of window class `class` whose text
    /// contains `containing`, when given.
    fn find_child(parent: HWND, class: &str, containing: Option<&str>) -> Option<HWND> {
        struct Search {
            class: String,
            containing: Option<String>,
            found: Option<HWND>,
        }
        unsafe extern "system" fn visit(hwnd: HWND, search: LPARAM) -> i32 {
            // SAFETY: `search` is the `Search` passed to `EnumChildWindows`
            // below, alive for the whole enumeration.
            let search = unsafe { &mut *(search as *mut Search) };
            let mut class = [0u16; 32];
            // SAFETY: `hwnd` is the live window being enumerated.
            let length = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), 32) };
            let class = String::from_utf16_lossy(&class[..usize::try_from(length).unwrap_or(0)]);
            if class.eq_ignore_ascii_case(&search.class)
                && search
                    .containing
                    .as_ref()
                    .is_none_or(|part| text_of(hwnd).contains(part.as_str()))
            {
                search.found = Some(hwnd);
                return 0;
            }
            1
        }
        let mut search = Search {
            class: class.to_owned(),
            containing: containing.map(str::to_owned),
            found: None,
        };
        // SAFETY: `parent` is a live window; `visit` only reads the
        // `Search` it is given, which outlives the call.
        unsafe { EnumChildWindows(parent, Some(visit), (&raw mut search) as LPARAM) };
        search.found
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
}

/// Pins the process to its first core: the low-end profile.
fn pin_to_one_core() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessAffinityMask};
        // SAFETY: the current process's pseudo-handle; a one-core mask.
        let pinned = unsafe { SetProcessAffinityMask(GetCurrentProcess(), 1) } != 0;
        assert!(pinned, "SetProcessAffinityMask failed");
    }
    // Elsewhere the harness is run under `taskset -c 0`.
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let scenario = args
        .iter()
        .position(|arg| arg == "--scenario")
        .and_then(|index| args.get(index + 1))
        .map_or("core", String::as_str);
    if args.iter().any(|arg| arg == "--low-end") {
        pin_to_one_core();
    }
    let measured = match scenario {
        "core" => core(),
        "compile" => compile(),
        "headless" => headless(),
        #[cfg(windows)]
        "startup" => native::startup(),
        #[cfg(windows)]
        "interaction" => native::interaction(),
        #[cfg(windows)]
        "animation" => native::animation(),
        #[cfg(windows)]
        "filter" => native::filter(),
        other => {
            eprintln!("bench-app: no scenario `{other}` on this platform");
            return ExitCode::FAILURE;
        }
    };
    println!("{measured}");
    ExitCode::SUCCESS
}
