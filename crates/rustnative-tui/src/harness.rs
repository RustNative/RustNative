//! The synthetic terminal and the harness that drives it (`PLAN.md`
//! Milestone 38: "a deterministic harness that drives a synthetic terminal
//! of a given size and asserts the resulting cell grid"; one of Milestone
//! 45's headless test backends).
//!
//! The harness runs the real [`Session`] — the same loop the platform
//! runs — against a [`SyntheticTerminal`] whose screen is a VT emulator fed
//! the bytes the session actually writes. Input goes in as the bytes a
//! terminal would send (a key's escape sequence, an SGR mouse report, a
//! bracketed paste), so the decoder is exercised too. Time is virtual: a
//! [`ManualExecutor`] runs every task and delay, and frames, the Escape
//! timeout, and the probe's deadline come due only when [`TuiHarness::advance`]
//! moves the clock.
//!
//! ```
//! use rustnative_core::{Component, Event, Node, NodeId, Size, Window};
//! use rustnative_tui::TuiHarness;
//!
//! struct Counter(u32);
//! impl Component for Counter {
//!     type Props = ();
//!     type Message = ();
//!     fn new((): ()) -> Self { Self(0) }
//!     fn props(&self) -> &() { &() }
//!     fn set_props(&mut self, (): ()) {}
//!     fn view(&self) -> Node {
//!         Node::column("root", [
//!             Node::label("count", format!("Count: {}", self.0)),
//!             Node::button("more", "More"),
//!         ])
//!     }
//!     fn update(&mut self, event: Event) {
//!         if matches!(event, Event::Click { target } if target == NodeId::from_key("more")) {
//!             self.0 += 1;
//!         }
//!     }
//! }
//!
//! let mut terminal = TuiHarness::launch(Window::new("Counter", Size::new(0, 0)), 40, 10, || Counter::new(()));
//! terminal.click_key("more");
//! assert!(terminal.text().contains("Count: 1"));
//! terminal.press_tab();
//! terminal.press_enter();
//! assert!(terminal.text().contains("Count: 2"));
//! # let _ = rustnative_core::rsx! { <Label key="x" text="x" /> };
//! ```

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rustnative_core::{
    Application, Component, Executor, KeyCode, KeyModifiers, ManualExecutor, MemoryStateStore,
    NodeId, Services, StateStore, Theme, Window, WindowId,
};

use crate::emulator::{Answers, Emulator};
use crate::grid::Grid;
use crate::model::{RealizedNode, TuiModel};
use crate::services::{Channel, Osc52Clipboard, TerminalSystem};
use crate::session::{Session, SessionStats, TuiConfig};
use crate::term::{Readiness, Signal, Terminal, Waker};

#[derive(Debug)]
struct State {
    emulator: Emulator,
    input: VecDeque<u8>,
    resized: bool,
    signals: Vec<Signal>,
    woken: bool,
    written: Vec<u8>,
    name: String,
}

/// A terminal that exists only in memory: a VT emulator for output, a queue
/// for input.
#[derive(Debug, Clone)]
pub struct SyntheticTerminal {
    state: Arc<Mutex<State>>,
}

impl SyntheticTerminal {
    /// A terminal of `columns` × `rows`, answering questions as `answers`.
    #[must_use]
    pub fn new(columns: u16, rows: u16, answers: Answers) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                emulator: Emulator::new(columns, rows, answers),
                input: VecDeque::new(),
                resized: false,
                signals: Vec::new(),
                woken: false,
                written: Vec::new(),
                name: "synthetic".to_owned(),
            })),
        }
    }

    /// Queues bytes as the terminal would send them.
    pub fn send(&self, bytes: &[u8]) {
        self.state.lock().input.extend(bytes);
    }

    /// Resizes it, as a person dragging its window would.
    pub fn resize(&self, columns: u16, rows: u16) {
        let mut state = self.state.lock();
        state.emulator.resize(columns, rows);
        state.resized = true;
    }

    /// Delivers `signal`.
    pub fn signal(&self, signal: Signal) {
        self.state.lock().signals.push(signal);
    }

    /// Runs `f` over the emulator.
    pub fn with_emulator<R>(&self, f: impl FnOnce(&mut Emulator) -> R) -> R {
        f(&mut self.state.lock().emulator)
    }

    /// Takes the bytes written since the last call.
    pub fn take_written(&self) -> Vec<u8> {
        std::mem::take(&mut self.state.lock().written)
    }
}

impl Terminal for SyntheticTerminal {
    fn size(&self) -> (u16, u16) {
        let state = self.state.lock();
        let screen = state.emulator.screen();
        (screen.columns(), screen.rows())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut state = self.state.lock();
        state.written.extend_from_slice(bytes);
        state.emulator.feed(bytes);
        let replies = state.emulator.take_replies();
        state.input.extend(replies);
        Ok(())
    }

    fn wait(&mut self, _timeout: Option<Duration>) -> io::Result<Readiness> {
        let mut state = self.state.lock();
        Ok(Readiness {
            input: state.input.drain(..).collect(),
            resized: std::mem::take(&mut state.resized),
            signals: std::mem::take(&mut state.signals),
            woken: std::mem::take(&mut state.woken),
        })
    }

    fn waker(&self) -> Waker {
        let state = Arc::clone(&self.state);
        Arc::new(move || state.lock().woken = true)
    }

    fn name(&self) -> String {
        self.state.lock().name.clone()
    }
}

/// The bytes an xterm-compatible terminal sends for `key` with
/// `modifiers` (with no kitty protocol in force).
#[must_use]
pub fn key_bytes(key: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    let code = 1
        + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.ctrl)
        + 8 * u8::from(modifiers.meta);
    let csi = |final_byte: char| {
        if code == 1 { format!("\x1b[{final_byte}") } else { format!("\x1b[1;{code}{final_byte}") }
    };
    let tilde = |number: u8| {
        if code == 1 { format!("\x1b[{number}~") } else { format!("\x1b[{number};{code}~") }
    };
    let alt =
        |bytes: Vec<u8>| if modifiers.alt { [b"\x1b".as_slice(), &bytes].concat() } else { bytes };
    let text = match key {
        KeyCode::ArrowUp => csi('A'),
        KeyCode::ArrowDown => csi('B'),
        KeyCode::ArrowRight => csi('C'),
        KeyCode::ArrowLeft => csi('D'),
        KeyCode::Home => csi('H'),
        KeyCode::End => csi('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::Function(n @ 1..=4) => {
            let final_byte = char::from(b'P' + n - 1);
            if code == 1 {
                format!("\x1bO{final_byte}")
            } else {
                format!("\x1b[1;{code}{final_byte}")
            }
        }
        KeyCode::Function(n) => {
            let number = match n {
                5 => 15,
                6..=10 => n + 11,
                11..=14 => n + 12,
                15..=16 => n + 13,
                _ => n + 14,
            };
            tilde(number)
        }
        KeyCode::Tab if modifiers.shift => "\x1b[Z".to_owned(),
        KeyCode::Tab => return alt(b"\t".to_vec()),
        KeyCode::Enter => return alt(b"\r".to_vec()),
        KeyCode::Escape => return alt(b"\x1b".to_vec()),
        KeyCode::Backspace if modifiers.ctrl => return alt(vec![0x08]),
        KeyCode::Backspace => return alt(vec![0x7F]),
        KeyCode::Space if modifiers.ctrl => return alt(vec![0]),
        KeyCode::Space => return alt(b" ".to_vec()),
        KeyCode::Character(c) if modifiers.ctrl && c.is_ascii_alphabetic() => {
            return alt(vec![u8::try_from(c.to_ascii_lowercase()).unwrap_or(b'a') - b'a' + 1]);
        }
        KeyCode::Character(c) => {
            let c = if modifiers.shift { c.to_uppercase().next().unwrap_or(c) } else { c };
            return alt(c.to_string().into_bytes());
        }
        _ => String::new(),
    };
    text.into_bytes()
}

/// A terminal application on a synthetic terminal, driven as a person
/// drives one.
pub struct TuiHarness {
    session: Session<SyntheticTerminal>,
    terminal: SyntheticTerminal,
    executor: ManualExecutor,
    state: Arc<MemoryStateStore>,
}

impl std::fmt::Debug for TuiHarness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TuiHarness").field("stats", &self.session.stats()).finish_non_exhaustive()
    }
}

impl TuiHarness {
    /// Launches `root()` in a `columns` × `rows` terminal that answers like
    /// a modern one ([`Answers::modern`]).
    pub fn launch<C, F>(window: Window, columns: u16, rows: u16, root: F) -> Self
    where
        C: Component,
        F: FnOnce() -> C,
    {
        Self::launch_with(window, columns, rows, Answers::modern(), Theme::default(), root)
    }

    /// Launches against a terminal answering as `answers`, with `theme`.
    pub fn launch_with<C, F>(
        window: Window,
        columns: u16,
        rows: u16,
        answers: Answers,
        theme: Theme,
        root: F,
    ) -> Self
    where
        C: Component,
        F: FnOnce() -> C,
    {
        let terminal = SyntheticTerminal::new(columns, rows, answers);
        let executor = ManualExecutor::new();
        let channel = Channel::new();
        let state = Arc::new(MemoryStateStore::new());
        let services = Services::default()
            .with_clock(Arc::new(executor.clone()))
            .with_clipboard(Arc::new(Osc52Clipboard::new(Arc::clone(&channel))))
            .with_system(Arc::new(TerminalSystem::new(Arc::clone(&channel), "harness")))
            .with_state_store(Arc::clone(&state) as Arc<dyn StateStore>);
        let app = Application::with_executor(
            root(),
            window,
            services,
            theme,
            Arc::new(executor.clone()) as Arc<dyn Executor>,
        );
        let config = TuiConfig { suspend_key: false, ..TuiConfig::default() };
        // The same environment on every machine: a true-colour xterm, no
        // locale, no reduced motion.
        let environment = crate::color::DetectInput {
            vars: vec![
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
            ],
            windows_vt: false,
        };
        let mut session = Session::with_environment(
            terminal.clone(),
            app,
            Arc::new(executor.clone()),
            config,
            channel,
            &environment,
        );
        session.start();
        let mut harness = Self { session, terminal, executor, state };
        harness.settle();
        // A terminal that answers nothing is waited for until the deadline.
        if harness.session.is_probing() {
            harness.advance(crate::probe::deadline());
        }
        harness
    }

    /// Runs everything that is ready and writes the frame it leads to —
    /// what the loop does between two inputs.
    pub fn settle(&mut self) {
        for _ in 0..100 {
            self.executor.run_until_stalled();
            self.session.step(Some(Duration::ZERO));
            self.executor.run_until_stalled();
            self.session.after_input();
            if self.session.is_finished() {
                return;
            }
            // Frames are paced in real use; here every settled change is
            // drawn, so a test sees the screen its input led to.
            self.session.frame_if_dirty();
            let idle = self.terminal.state.lock().input.is_empty()
                && self.executor.pending_task_count() == 0;
            if idle && !self.session.is_dirty() {
                break;
            }
        }
    }

    /// Moves virtual time forward by `duration` in frame-sized steps,
    /// running what comes due, then settles.
    pub fn advance(&mut self, duration: Duration) {
        let step = Duration::from_millis(10);
        let mut left = duration;
        while !left.is_zero() {
            let now = left.min(step);
            self.executor.advance(now);
            left -= now;
            self.session.after_input();
            self.executor.run_until_stalled();
        }
        self.settle();
    }

    /// Sends raw bytes as the terminal would, and settles.
    pub fn send(&mut self, bytes: &[u8]) {
        self.terminal.send(bytes);
        self.settle();
    }

    /// Presses `key` with `modifiers`.
    pub fn press(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        self.send(&key_bytes(key, modifiers));
        // A lone Escape waits for its timeout, as on a real terminal.
        if self.session_has_pending_escape() {
            self.advance(crate::decode::ESCAPE_TIMEOUT + Duration::from_millis(1));
        }
    }

    fn session_has_pending_escape(&self) -> bool {
        self.session.has_pending_input()
    }

    /// Presses Tab.
    pub fn press_tab(&mut self) {
        self.press(KeyCode::Tab, KeyModifiers::default());
    }

    /// Presses Enter.
    pub fn press_enter(&mut self) {
        self.press(KeyCode::Enter, KeyModifiers::default());
    }

    /// Types `text`, a character at a time.
    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            let mut buffer = [0_u8; 4];
            self.send(character.encode_utf8(&mut buffer).as_bytes());
        }
    }

    /// Pastes `text` (bracketed).
    pub fn paste(&mut self, text: &str) {
        self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
    }

    /// Clicks cell (`column`, `row`) with the primary button.
    pub fn click(&mut self, column: u16, row: u16) {
        self.send(
            format!("\x1b[<0;{};{}M\x1b[<0;{};{}m", column + 1, row + 1, column + 1, row + 1)
                .as_bytes(),
        );
    }

    /// Drags with the primary button from one cell to another.
    pub fn drag(&mut self, from: (u16, u16), to: (u16, u16)) {
        self.send(format!("\x1b[<0;{};{}M", from.0 + 1, from.1 + 1).as_bytes());
        self.send(format!("\x1b[<32;{};{}M", to.0 + 1, to.1 + 1).as_bytes());
        self.send(format!("\x1b[<0;{};{}m", to.0 + 1, to.1 + 1).as_bytes());
    }

    /// Moves the pointer to a cell with no button held.
    pub fn hover(&mut self, column: u16, row: u16) {
        self.send(format!("\x1b[<35;{};{}M", column + 1, row + 1).as_bytes());
    }

    /// Turns the wheel one notch over a cell: `down` toward the end.
    pub fn wheel(&mut self, column: u16, row: u16, down: bool) {
        let code = if down { 65 } else { 64 };
        self.send(format!("\x1b[<{code};{};{}M", column + 1, row + 1).as_bytes());
    }

    /// Clicks the centre of the node keyed `key` in the shown window.
    ///
    /// # Panics
    ///
    /// If no shown node has that key.
    pub fn click_key(&mut self, key: &str) {
        let node = self.node(key).unwrap_or_else(|| panic!("no node keyed `{key}` is shown"));
        let cells = node.visible_cells;
        let column = u16::try_from(cells.x + cells.width / 2).unwrap_or(0);
        let row = u16::try_from(cells.y + cells.height / 2).unwrap_or(0);
        self.click(column, row);
    }

    /// Resizes the terminal.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.terminal.resize(columns, rows);
        self.settle();
    }

    /// Delivers `signal`.
    pub fn signal(&mut self, signal: Signal) {
        self.terminal.signal(signal);
        self.settle();
    }

    /// The session.
    #[must_use]
    pub const fn session(&self) -> &Session<SyntheticTerminal> {
        &self.session
    }

    /// The session, mutably.
    pub fn session_mut(&mut self) -> &mut Session<SyntheticTerminal> {
        &mut self.session
    }

    /// The synthetic terminal.
    #[must_use]
    pub const fn terminal(&self) -> &SyntheticTerminal {
        &self.terminal
    }

    /// The executor.
    #[must_use]
    pub const fn executor(&self) -> &ManualExecutor {
        &self.executor
    }

    /// The state store.
    #[must_use]
    pub fn state_store(&self) -> &MemoryStateStore {
        &self.state
    }

    /// The application.
    #[must_use]
    pub const fn application(&self) -> &Application {
        self.session.application()
    }

    /// The application, mutably (then [`Self::settle`]).
    pub const fn application_mut(&mut self) -> &mut Application {
        self.session.application_mut()
    }

    /// What the terminal shows: the emulated screen.
    #[must_use]
    pub fn screen(&self) -> Grid {
        self.terminal.with_emulator(|emulator| emulator.screen().clone())
    }

    /// The screen's text, trailing blanks trimmed.
    #[must_use]
    pub fn text(&self) -> String {
        self.screen().text()
    }

    /// The cursor's cell, and whether it is shown.
    #[must_use]
    pub fn cursor(&self) -> ((i32, i32), bool) {
        self.terminal.with_emulator(|emulator| (emulator.cursor(), emulator.mode(25)))
    }

    /// The model of the shown window.
    ///
    /// # Panics
    ///
    /// If no window is shown.
    #[must_use]
    pub fn model(&self) -> &TuiModel {
        let id = self.session.shown_window().unwrap_or(WindowId::PRIMARY);
        self.session.model(id).unwrap_or_else(|| panic!("window {id:?} is not realized"))
    }

    /// The shown node keyed `key`.
    #[must_use]
    pub fn node(&self, key: &str) -> Option<RealizedNode> {
        let id = NodeId::from_key(key);
        self.model().nodes().find(|node| node.id == id || node.key.as_deref() == Some(key)).cloned()
    }

    /// The focused node's key.
    #[must_use]
    pub fn focused_key(&self) -> Option<String> {
        self.model().focused().and_then(|id| self.model().get(id)).and_then(|node| node.key.clone())
    }

    /// Counters.
    #[must_use]
    pub const fn stats(&self) -> SessionStats {
        self.session.stats()
    }

    /// Bytes written since the last call.
    pub fn take_written(&mut self) -> Vec<u8> {
        self.terminal.take_written()
    }

    /// A stable description of the screen for a golden file: the text, and
    /// a line per row naming which cells are reversed (`R`), bold (`B`),
    /// underlined (`U`), or faint (`F`).
    #[must_use]
    pub fn golden(&self) -> String {
        use crate::grid::Attrs;
        let screen = self.screen();
        let mut out = String::new();
        for y in 0..screen.rows() {
            let text = screen.row_text(y);
            let marks: String = screen
                .row(y)
                .iter()
                .filter(|cell| !cell.is_continuation())
                .map(|cell| {
                    let attrs = cell.style.attrs;
                    if attrs.has(Attrs::REVERSE) {
                        'R'
                    } else if attrs.has(Attrs::BOLD) {
                        'B'
                    } else if attrs.has(Attrs::UNDERLINE) {
                        'U'
                    } else if attrs.has(Attrs::DIM) {
                        'F'
                    } else {
                        ' '
                    }
                })
                .collect();
            let marks = marks.trim_end();
            out.push_str(&format!("{text}\n"));
            if !marks.is_empty() {
                out.push_str(&format!("{:>width$}\n", "", width = 0));
                out.pop();
                out.push_str(&format!("~{marks}\n"));
            }
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        let ((x, y), shown) = self.cursor();
        out.push_str(&format!("cursor {x},{y}{}\n", if shown { " shown" } else { "" }));
        out
    }
}
