//! The loop, independent of where its terminal is (`PLAN.md` Milestone 38,
//! "scheduling and redraw").
//!
//! One thread, input and redraw decoupled:
//!
//! ```text
//! wait        input, a resize, a signal, a task's wake, or the next
//!   │         deadline (a frame, the Escape timeout, the probe, a read)
//!   ▼
//! input       decoded into keys, paste, mouse, focus, and probe answers;
//!   │         each becomes host behaviour (focus, editing, activation)
//!   │         and the portable events it implies, dispatched in order
//!   ▼
//! tasks       completed tasks delivered; inspection requests answered
//!   ▼
//! frame       at most one per frame interval (30 Hz), only when something
//!             changed: realize, paint, and write the damage
//! ```
//!
//! [`Session`] is used by [`crate::TuiPlatform`] on the process's own
//! terminal and by [`crate::TuiHarness`] on a synthetic one, so what the
//! tests drive is the loop that runs.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustnative_core::{
    AnimatedProperty, AnimatedValue, AnimationOwner, AnimationRequest, Application, Capability,
    Clock, ColorScheme, Control, Event, KeyCode, Lifecycle, MotionPreference, NodeId, NodeKind,
    PanicAction, PanicReport, PlatformCapabilities, Point, PointerButton, PointerButtons,
    PointerEvent, PointerKind, Scalar, Size, Theme, Timeline, VirtualRange, WheelDelta, WindowId,
};
use rustnative_drawlist::GridMapping;

use crate::color::{ColorDepth, DetectInput};
use crate::decode::{
    Decoder, ESCAPE_TIMEOUT, InputEvent, Key, KeyKind, Mouse, MouseButton, MouseKind, Reply,
};
use crate::edit;
use crate::encode::{Encoder, seq, write_diff, write_full};
use crate::grid::Grid;
use crate::measure::TuiMeasurer;
use crate::model::TuiModel;
use crate::paint::{self, CursorSpot, PaintContext};
use crate::probe::{self, ClipboardSupport, TerminalProfile};
use crate::services::Channel;
use crate::term::{Readiness, Signal, Terminal};
use crate::width::Width;

/// The default frame interval: 30 frames a second.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Lines a wheel notch scrolls.
pub const WHEEL_LINES: i32 = 3;

/// How the session behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "independent switches, each documented")]
pub struct TuiConfig {
    /// Logical pixels per cell.
    pub mapping: GridMapping,
    /// The shortest time between two frames.
    pub frame_interval: Duration,
    /// Whether mouse reporting is requested.
    pub mouse: bool,
    /// Whether the session takes the alternate screen (and leaves the
    /// shell's screen as it was).
    pub alternate_screen: bool,
    /// Whether Ctrl+Z suspends (Unix job control), unless a command takes
    /// it.
    pub suspend_key: bool,
    /// Whether the terminal is asked what it supports at startup.
    pub probe: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            mapping: GridMapping::new(8, 16),
            frame_interval: FRAME_INTERVAL,
            mouse: true,
            alternate_screen: true,
            suspend_key: cfg!(unix),
            probe: true,
        }
    }
}

impl TuiConfig {
    /// The defaults, overridden by the environment: `RUSTNATIVE_TUI_FPS`
    /// (frames a second, 1–240) and `RUSTNATIVE_TUI_MOUSE=0`.
    #[must_use]
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Some(fps) =
            std::env::var("RUSTNATIVE_TUI_FPS").ok().and_then(|value| value.parse::<u64>().ok())
        {
            config.frame_interval = Duration::from_millis(1000 / fps.clamp(1, 240));
        }
        if std::env::var("RUSTNATIVE_TUI_MOUSE").is_ok_and(|value| value == "0") {
            config.mouse = false;
        }
        config
    }
}

/// Counters a budget or a test reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionStats {
    /// Bytes written to the terminal.
    pub bytes_written: u64,
    /// Frames written.
    pub frames: u64,
    /// Frames that redrew everything.
    pub full_redraws: u64,
    /// Cells written.
    pub cells_written: u64,
}

/// Why a session ended badly.
#[derive(Debug)]
pub enum SessionError {
    /// The terminal failed.
    Io(io::Error),
    /// A component panicked and the policy said to stop.
    ComponentPanicked(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "the terminal failed: {error}"),
            Self::ComponentPanicked(message) => write!(f, "a component panicked: {message}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The capabilities a terminal session realizes, given what the terminal
/// answered (`PLAN.md` Milestone 38's list; `docs/tui.md`).
#[must_use]
pub fn capabilities_for(profile: &TerminalProfile) -> PlatformCapabilities {
    let mut set = vec![
        Capability::CommandShortcuts,
        Capability::StatePersistence,
        Capability::Lifecycle,
        Capability::RightToLeft,
        Capability::Animations,
        Capability::ReducedMotionPreference,
        Capability::CustomDrawing,
        Capability::Storage,
    ];
    if profile.clipboard != ClipboardSupport::None {
        set.push(Capability::Clipboard);
    }
    if profile.notifications.is_some() {
        set.push(Capability::Notifications);
    }
    if crate::services::url_opener(profile.remote).is_some() {
        set.push(Capability::UrlLaunch);
    }
    if profile.reports_hover() {
        set.push(Capability::Hover);
    }
    PlatformCapabilities::new(set)
}

#[derive(Debug, Clone, Copy)]
struct Press {
    window: WindowId,
    node: NodeId,
    button: MouseButton,
}

/// A running terminal application.
pub struct Session<T: Terminal> {
    pub(crate) terminal: T,
    pub(crate) app: Application,
    clock: Arc<dyn Clock>,
    config: TuiConfig,
    decoder: Decoder,
    pending_since: Option<Duration>,
    encoder: Encoder,
    profile: TerminalProfile,
    probe_until: Option<Duration>,
    width: Width,
    models: HashMap<WindowId, TuiModel>,
    stack: Vec<WindowId>,
    shown: Grid,
    full_redraw: bool,
    dirty: bool,
    last_frame: Option<Duration>,
    timelines: HashMap<WindowId, Timeline>,
    animated: HashMap<WindowId, HashMap<(NodeId, AnimatedProperty), AnimatedValue>>,
    channel: Arc<Channel>,
    press: Option<Press>,
    pointer_capture: Option<(NodeId, u32)>,
    reported_ranges: HashMap<(WindowId, NodeId), VirtualRange>,
    paste_pending: Option<(WindowId, NodeId)>,
    stats: SessionStats,
    quit: bool,
    failure: Option<SessionError>,
    entered: bool,
    motion: MotionPreference,
    cursor: Option<CursorSpot>,
    baseline: Theme,
    titles: HashMap<WindowId, String>,
    capabilities: PlatformCapabilities,
    last_press_column: Option<i32>,
}

fn reduced_motion_from_env() -> MotionPreference {
    let set = |name: &str| std::env::var(name).is_ok_and(|value| !value.is_empty() && value != "0");
    if set("NO_MOTION") || set("RUSTNATIVE_REDUCED_MOTION") {
        MotionPreference::Reduced
    } else {
        MotionPreference::Full
    }
}

fn locale_from_env() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty() && value != "C" && value != "POSIX")
            .map(|value| value.split('.').next().unwrap_or(&value).replace('_', "-"))
    })
}

impl<T: Terminal> Session<T> {
    /// A session running `app` on `terminal`, timed by `clock`.
    pub fn new(
        terminal: T,
        app: Application,
        clock: Arc<dyn Clock>,
        config: TuiConfig,
        channel: Arc<Channel>,
    ) -> Self {
        let input = DetectInput::from_process(terminal.is_windows_console());
        let profile = TerminalProfile::from_environment(&input, terminal.name());
        let width = Width::for_locale(&locale_from_env().unwrap_or_default());
        let (columns, rows) = terminal.size();
        let capabilities = capabilities_for(&profile);
        channel.set_profile(profile.clone());
        channel.set_waker(terminal.waker());
        Self {
            terminal,
            app,
            clock,
            config,
            decoder: Decoder::new(),
            pending_since: None,
            encoder: Encoder::new(),
            profile,
            probe_until: None,
            width,
            models: HashMap::new(),
            stack: Vec::new(),
            shown: Grid::new(columns, rows),
            full_redraw: true,
            dirty: true,
            last_frame: None,
            timelines: HashMap::new(),
            animated: HashMap::new(),
            channel,
            press: None,
            pointer_capture: None,
            reported_ranges: HashMap::new(),
            paste_pending: None,
            stats: SessionStats::default(),
            quit: false,
            failure: None,
            entered: false,
            motion: reduced_motion_from_env(),
            cursor: None,
            baseline: Theme::default(),
            titles: HashMap::new(),
            capabilities,
            last_press_column: None,
        }
    }

    /// Overrides the colour depth detection found (a test of a depth).
    pub fn set_color_depth(&mut self, depth: ColorDepth) {
        self.profile.color_depth = depth;
        self.full_redraw = true;
        self.dirty = true;
    }

    /// Overrides the motion preference (a test of reduced motion).
    pub fn set_motion_preference(&mut self, motion: MotionPreference) {
        self.motion = motion;
        self.app.set_motion_preference(motion);
        for timeline in self.timelines.values_mut() {
            timeline.set_motion_preference(motion);
        }
        self.dirty = true;
    }

    /// The application.
    pub const fn application(&self) -> &Application {
        &self.app
    }

    /// The application, mutably.
    pub const fn application_mut(&mut self) -> &mut Application {
        &mut self.app
    }

    /// The terminal.
    pub const fn terminal(&self) -> &T {
        &self.terminal
    }

    /// The terminal, mutably.
    pub const fn terminal_mut(&mut self) -> &mut T {
        &mut self.terminal
    }

    /// What is known about the terminal.
    pub const fn profile(&self) -> &TerminalProfile {
        &self.profile
    }

    /// The capabilities advertised now.
    pub const fn capabilities(&self) -> &PlatformCapabilities {
        &self.capabilities
    }

    /// Counters.
    pub const fn stats(&self) -> SessionStats {
        self.stats
    }

    /// The window shown (the top of the stack).
    pub fn shown_window(&self) -> Option<WindowId> {
        self.stack.last().copied()
    }

    /// The realized model of window `id`.
    pub fn model(&self, id: WindowId) -> Option<&TuiModel> {
        self.models.get(&id)
    }

    /// Every realized window.
    pub const fn models(&self) -> &HashMap<WindowId, TuiModel> {
        &self.models
    }

    /// The grid on screen.
    pub const fn shown_grid(&self) -> &Grid {
        &self.shown
    }

    /// The cell size.
    pub const fn mapping(&self) -> GridMapping {
        self.config.mapping
    }

    /// The width rules.
    pub const fn width(&self) -> Width {
        self.width
    }

    /// The services' channel.
    pub fn channel(&self) -> &Arc<Channel> {
        &self.channel
    }

    /// Whether the session has ended.
    pub const fn is_finished(&self) -> bool {
        self.quit
    }

    /// Whether something changed that the next frame will draw.
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether the terminal's answers are still awaited.
    pub const fn is_probing(&self) -> bool {
        self.probe_until.is_some()
    }

    /// Whether input bytes wait for the rest of a sequence (a lone `ESC`).
    pub const fn has_pending_input(&self) -> bool {
        self.pending_since.is_some()
    }

    /// Writes a frame now if anything changed, regardless of pacing.
    pub fn frame_if_dirty(&mut self) {
        if !self.quit && (self.dirty || self.full_redraw) {
            self.frame();
        }
    }

    /// Takes the failure that ended the session, if one did.
    pub fn take_failure(&mut self) -> Option<SessionError> {
        self.failure.take()
    }

    fn now(&self) -> Duration {
        self.clock.now()
    }

    fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.stats.bytes_written += bytes.len() as u64;
        if let Err(error) = self.terminal.write(bytes) {
            self.fail(SessionError::Io(error));
        }
    }

    fn fail(&mut self, error: SessionError) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
        self.quit = true;
    }

    fn enter_sequences(&self) -> String {
        let mut out = String::new();
        out.push_str(seq::PUSH_TITLE);
        if self.config.alternate_screen {
            out.push_str(seq::ENTER_ALT_SCREEN);
        }
        out.push_str(seq::HIDE_CURSOR);
        out.push_str(seq::AUTOWRAP_OFF);
        out.push_str(seq::PASTE_ON);
        out.push_str(seq::FOCUS_ON);
        if self.config.mouse {
            out.push_str(seq::MOUSE_ON);
        }
        if self.profile.kitty_keyboard {
            out.push_str(seq::KITTY_KEYS_ON);
        }
        out
    }

    /// Takes the terminal: the modes, the probe, the first size, the
    /// environment, and the wakes. Called once before the first step.
    pub fn start(&mut self) {
        let _ = self.app.enable_inspection_from_env();
        let mut out = self.enter_sequences();
        if self.config.probe {
            out.push_str(&probe::questions());
            self.probe_until = Some(self.now() + probe::deadline());
        }
        self.write(out.as_bytes());
        self.entered = true;
        self.app.set_motion_preference(self.motion);
        if let Some(locale) = locale_from_env() {
            self.app.set_locale(rustnative_core::Locale::new(locale));
        }
        self.app.set_environment(&rustnative_core::keys::CAPABILITIES, self.capabilities.clone());
        self.resized();
        self.sync_windows();
        self.dirty = true;
    }

    fn sync_windows(&mut self) {
        let open = self.app.window_ids();
        self.stack.retain(|id| open.contains(id));
        let mut new: Vec<WindowId> =
            open.iter().copied().filter(|id| !self.stack.contains(id)).collect();
        new.sort_by_key(|id| id.get());
        let waker = self.terminal.waker();
        let size = self.window_size();
        for id in new {
            self.stack.push(id);
            if let Some(scheduler) = self.app.scheduler_for(id) {
                let waker = Arc::clone(&waker);
                scheduler.set_waker(Arc::new(move || waker()));
            }
            let mut timeline = Timeline::new();
            timeline.set_motion_preference(self.motion);
            self.timelines.insert(id, timeline);
            self.app.dispatch_to_window(id, Event::WindowResized { window: id, size });
            self.full_redraw = true;
        }
        self.models.retain(|id, _| open.contains(id));
        self.timelines.retain(|id, _| open.contains(id));
        self.animated.retain(|id, _| open.contains(id));
        if self.stack.is_empty() {
            self.quit = true;
        }
    }

    fn window_size(&self) -> Size {
        let (columns, rows) = self.terminal.size();
        self.config.mapping.logical_size(columns, rows)
    }

    fn resized(&mut self) {
        let (columns, rows) = self.terminal.size();
        self.shown.resize(columns, rows);
        self.full_redraw = true;
        self.dirty = true;
        let size = self.config.mapping.logical_size(columns, rows);
        for id in self.app.window_ids() {
            self.app.dispatch_to_window(id, Event::WindowResized { window: id, size });
        }
    }

    /// When the loop must wake next, from now: a frame due, the Escape
    /// timeout, the probe's deadline, or a clipboard read's.
    #[must_use]
    pub fn next_timeout(&self) -> Option<Duration> {
        let now = self.now();
        let mut due: Option<Duration> = None;
        let mut consider = |at: Duration| due = Some(due.map_or(at, |due| due.min(at)));
        if self.dirty || self.animating() {
            consider(self.last_frame.map_or(now, |last| last + self.config.frame_interval));
        }
        if let Some(since) = self.pending_since {
            consider(since + ESCAPE_TIMEOUT);
        }
        if let Some(until) = self.probe_until {
            consider(until);
        }
        if let Some(deadline) = self.channel.next_read_deadline() {
            let left = deadline.saturating_duration_since(Instant::now());
            consider(now + left);
        }
        due.map(|due| due.saturating_sub(now))
    }

    fn animating(&self) -> bool {
        self.timelines.values().any(Timeline::is_active) || self.indeterminate_progress_shown()
    }

    fn indeterminate_progress_shown(&self) -> bool {
        self.motion == MotionPreference::Full
            && self.shown_window().and_then(|id| self.models.get(&id)).is_some_and(|model| {
                model.nodes().any(|node| {
                    matches!(node.control, Some(Control::Progress { percent: None }))
                        && !node.hidden
                        && !node.visible_cells.is_empty()
                })
            })
    }

    /// One turn of the loop: wait (at most `limit`), handle what happened,
    /// run tasks, and write a frame if one is due.
    pub fn step(&mut self, limit: Option<Duration>) {
        let timeout = match (self.next_timeout(), limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match self.terminal.wait(timeout) {
            Ok(readiness) => self.handle(readiness),
            Err(error) => self.fail(SessionError::Io(error)),
        }
        self.after_input();
    }

    /// Handles what a wait found.
    pub fn handle(&mut self, readiness: Readiness) {
        if readiness.resized {
            self.resized();
        }
        for signal in readiness.signals {
            self.signal(signal);
        }
        if !readiness.input.is_empty() {
            let events = self.decoder.feed(&readiness.input);
            self.pending_since = self.decoder.has_pending().then(|| self.now());
            for event in events {
                self.input(event);
            }
        }
        if readiness.woken {
            self.dirty = true;
        }
    }

    /// Everything after input: the Escape timeout, the probe's deadline,
    /// tasks, inspection, queued service output, and the frame.
    pub fn after_input(&mut self) {
        let now = self.now();
        if self.pending_since.is_some_and(|since| now >= since + ESCAPE_TIMEOUT) {
            self.pending_since = None;
            for event in self.decoder.flush() {
                self.input(event);
            }
        }
        if self.probe_until.is_some_and(|until| now >= until) {
            self.finish_probe();
        }
        self.channel.expire_reads(Instant::now());
        self.pump();
        let outbox = self.channel.take_outbox();
        self.write(&outbox);
        if self.quit {
            return;
        }
        let due = self.last_frame.is_none_or(|last| now >= last + self.config.frame_interval);
        if (self.dirty || self.animating()) && due {
            self.frame();
        }
    }

    fn finish_probe(&mut self) {
        self.probe_until = None;
        let before = self.profile.kitty_keyboard;
        self.channel.set_profile(self.profile.clone());
        self.capabilities = capabilities_for(&self.profile);
        self.app.set_environment(&rustnative_core::keys::CAPABILITIES, self.capabilities.clone());
        if let Some(dark) = self.profile.dark_background() {
            self.app.set_environment(
                &rustnative_core::keys::COLOR_SCHEME,
                if dark { ColorScheme::Dark } else { ColorScheme::Light },
            );
        }
        if self.profile.kitty_keyboard && !before {
            self.write(seq::KITTY_KEYS_ON.as_bytes());
        }
        // The probe can change the depth's answer, the scheme, and whether
        // frames are synchronized: everything is redrawn once.
        self.full_redraw = true;
        self.dirty = true;
    }

    fn signal(&mut self, signal: Signal) {
        match signal {
            Signal::Terminate | Signal::Hangup => {
                let _ = self.app.lifecycle(Lifecycle::Terminating);
                self.quit = true;
            }
            Signal::Suspend => self.suspend(),
            Signal::Continue => {
                let out = self.enter_sequences();
                self.write(out.as_bytes());
                self.encoder.invalidate();
                self.full_redraw = true;
                self.dirty = true;
                let _ = self.app.lifecycle(Lifecycle::Resuming);
            }
        }
    }

    fn suspend(&mut self) {
        let _ = self.app.lifecycle(Lifecycle::Suspending);
        if let Err(error) = self.terminal.suspend() {
            self.fail(SessionError::Io(error));
            return;
        }
        let out = self.enter_sequences();
        self.write(out.as_bytes());
        self.encoder.invalidate();
        self.resized();
        let _ = self.app.lifecycle(Lifecycle::Resuming);
    }

    // ------------------------------------------------------------------
    // Dispatch, inside the panic boundary
    // ------------------------------------------------------------------

    fn dispatch(&mut self, window: WindowId, event: Event) {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.app.dispatch_to_window(window, event)
        }));
        if let Err(payload) = outcome {
            self.panicked(window, panic_message(payload.as_ref()));
        }
        self.dirty = true;
    }

    fn panicked(&mut self, window: WindowId, message: String) {
        let report = PanicReport { message: message.clone(), window };
        match self.app.handle_component_panic(&report) {
            PanicAction::Terminate => self.fail(SessionError::ComponentPanicked(message)),
            PanicAction::CloseWindow(target) => {
                self.app.close_window(target);
                self.sync_windows();
            }
            _ => {}
        }
    }

    fn pump(&mut self) {
        for _ in 0..64 {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut changed = self.app.pump_tasks();
                if !changed {
                    changed = self.app.pump_deferred();
                }
                let inspected = {
                    let backend = crate::inspect::TuiInspect::new(
                        &self.models,
                        &self.capabilities,
                        self.config.mapping,
                    );
                    self.app.poll_inspection(&backend)
                };
                changed || inspected
            }));
            match outcome {
                Ok(true) => self.dirty = true,
                Ok(false) => break,
                Err(payload) => {
                    let window = self.shown_window().unwrap_or(WindowId::PRIMARY);
                    self.panicked(window, panic_message(payload.as_ref()));
                    break;
                }
            }
        }
        if self.app.window_ids().len() != self.stack.len()
            || self.app.window_ids().iter().any(|id| !self.stack.contains(id))
        {
            self.sync_windows();
        }
        self.collect_requests();
    }

    fn collect_requests(&mut self) {
        let now = self.now();
        for id in self.app.window_ids() {
            for request in self.app.take_input_requests(id) {
                match request {
                    rustnative_core::InputRequest::CapturePointer { node, pointer_id } => {
                        self.pointer_capture = Some((node, pointer_id));
                    }
                    rustnative_core::InputRequest::ReleasePointer { .. } => {
                        self.pointer_capture = None
                    }
                    _ => {}
                }
            }
            let requests = self.app.take_animation_requests(id);
            if requests.is_empty() {
                continue;
            }
            self.dirty = true;
            let model = self.models.get(&id);
            let timeline = self.timelines.entry(id).or_default();
            let overrides = self.animated.entry(id).or_default();
            for request in requests {
                match request {
                    AnimationRequest::Start { node, animation, owner } => {
                        let current = model.and_then(|model| {
                            current_value(model, &self.app, node, animation.property())
                        });
                        timeline.start(
                            node,
                            &animation,
                            AnimationOwner::Component(owner),
                            current,
                            now,
                        );
                    }
                    AnimationRequest::Cancel { node, property } => {
                        if let Some(running) = timeline.running_id(node, property) {
                            if let Some(frame) = timeline.cancel(running) {
                                apply_frame(overrides, &frame);
                            }
                        }
                    }
                    AnimationRequest::CancelOwner(owner) => {
                        for frame in timeline.cancel_owner(AnimationOwner::Component(owner)) {
                            apply_frame(overrides, &frame);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Realizing and drawing
    // ------------------------------------------------------------------

    fn realize_all(&mut self) {
        rustnative_core::perf::realized();
        let measurer = TuiMeasurer::new(self.config.mapping, self.width);
        let now = self.now();
        for id in self.app.window_ids() {
            let (Some(view), Some(state)) = (self.app.view_for(id), self.app.window_state(id))
            else {
                continue;
            };
            let size = state.size();
            let theme = self.app.theme().clone();
            let direction = self.app.layout_direction(id);
            let safe = self.app.environment_for(id, &rustnative_core::keys::SAFE_AREA);
            let model = self.models.entry(id).or_default();
            let before = transitioned_values(model, &theme);
            model.realize(&view, &theme, size, &measurer, self.config.mapping, direction, safe);
            let after = transitioned_values(model, &theme);
            let timeline = self.timelines.entry(id).or_default();
            for ((node, property), (to, transition)) in &after {
                if let Some((from, _)) = before.get(&(*node, *property)) {
                    if from != to {
                        let shown = self
                            .animated
                            .get(&id)
                            .and_then(|values| values.get(&(*node, *property)))
                            .copied();
                        timeline.transition(
                            *node,
                            *property,
                            shown.unwrap_or(*from),
                            *to,
                            *transition,
                            now,
                        );
                    }
                }
            }
            let live: Vec<NodeId> = model.nodes().map(|node| node.id).collect();
            if let Some(values) = self.animated.get_mut(&id) {
                values.retain(|(node, _), _| live.contains(node));
            }
        }
        self.report_ranges_and_sizes();
    }

    fn report_ranges_and_sizes(&mut self) {
        for _ in 0..4 {
            let mut changes = Vec::new();
            for (window, model) in &self.models {
                for node in model.nodes() {
                    let Some(style) = node.virtualization else { continue };
                    let extents = rustnative_core::ExtentCache::new(style.item_count, style.extent);
                    let (offset, viewport) = match style.axis {
                        rustnative_core::Axis::Vertical => (node.scroll.y, node.rect.height),
                        rustnative_core::Axis::Horizontal => (node.scroll.x, node.rect.width),
                    };
                    let range = VirtualRange::compute(
                        u32::try_from(offset.max(0)).unwrap_or(0),
                        u32::try_from(viewport.max(0)).unwrap_or(0),
                        &extents,
                        style.overscan,
                    );
                    if self.reported_ranges.get(&(*window, node.id)) != Some(&range) {
                        changes.push((*window, node.id, range));
                    }
                }
            }
            let mut sized = false;
            for id in self.app.window_ids() {
                let Some(model) = self.models.get(&id) else { continue };
                let sizes: Vec<_> = self
                    .app
                    .watched_nodes(id)
                    .into_iter()
                    .filter_map(|node| {
                        let rect = model.get(node)?.rect;
                        Some((
                            node,
                            Size::new(
                                u32::try_from(rect.width.max(0)).unwrap_or(0),
                                u32::try_from(rect.height.max(0)).unwrap_or(0),
                            ),
                        ))
                    })
                    .collect();
                sized |= self.app.report_sizes(id, sizes);
            }
            if changes.is_empty() && !sized {
                return;
            }
            for (window, target, range) in changes {
                self.reported_ranges.insert((window, target), range);
                self.dispatch(window, Event::VisibleRangeChanged { target, range });
            }
            let _ = self.app.pump_tasks();
            let measurer = TuiMeasurer::new(self.config.mapping, self.width);
            for id in self.app.window_ids() {
                let (Some(view), Some(state)) = (self.app.view_for(id), self.app.window_state(id))
                else {
                    continue;
                };
                let theme = self.app.theme().clone();
                let direction = self.app.layout_direction(id);
                let safe = self.app.environment_for(id, &rustnative_core::keys::SAFE_AREA);
                self.models.entry(id).or_default().realize(
                    &view,
                    &theme,
                    state.size(),
                    &measurer,
                    self.config.mapping,
                    direction,
                    safe,
                );
            }
        }
    }

    fn tick_animations(&mut self) {
        let now = self.now();
        let mut finished = Vec::new();
        for (id, timeline) in &mut self.timelines {
            if !timeline.is_active() {
                continue;
            }
            let output = timeline.tick(now);
            let overrides = self.animated.entry(*id).or_default();
            for frame in &output.frames {
                apply_frame(overrides, frame);
            }
            for done in output.finished {
                if matches!(done.owner, AnimationOwner::Component(_)) {
                    finished.push((*id, done.node, done.property));
                }
            }
        }
        for (window, target, property) in finished {
            self.dispatch(window, Event::AnimationFinished { target, property });
        }
    }

    /// Writes a frame now: realize, paint, and the damage.
    pub fn frame(&mut self) {
        let now = self.now();
        self.tick_animations();
        self.realize_all();
        let (columns, rows) = self.terminal.size();
        if self.shown.columns() != columns || self.shown.rows() != rows {
            self.shown.resize(columns, rows);
            self.full_redraw = true;
        }
        let mut next = Grid::new(columns, rows);
        let top = self.shown_window();
        let title =
            top.and_then(|id| self.app.window_for(id)).map(|window| window.title().to_owned());
        let mut cursor = None;
        if let Some(id) = top {
            let overrides = self.animated.get(&id).cloned().unwrap_or_default();
            let animated = move |node: NodeId, property: AnimatedProperty| {
                overrides.get(&(node, property)).copied()
            };
            let context = PaintContext {
                theme: &self.app.theme().clone(),
                baseline: &self.baseline,
                mapping: self.config.mapping,
                width: self.width,
                background: self.profile.background,
                now,
                reduced_motion: self.motion == MotionPreference::Reduced,
                has_color: self.profile.color_depth != ColorDepth::None,
                animated: &animated,
            };
            if let Some(model) = self.models.get_mut(&id) {
                cursor = paint::paint_window(model, &mut next, &context);
            }
        }
        let depth = self.profile.color_depth;
        let sync = self.profile.synchronized_output();
        if sync {
            self.encoder.raw(seq::BEGIN_SYNC);
        }
        if let (Some(id), Some(title)) = (top, title) {
            if self.titles.get(&id) != Some(&title) {
                self.titles.clear();
                self.titles.insert(id, title.clone());
                self.encoder.raw(&seq::title(&title));
            }
        }
        let cells = if self.full_redraw {
            self.stats.full_redraws += 1;
            write_full(&mut self.encoder, &next, depth)
        } else {
            write_diff(&mut self.encoder, &self.shown, &next, depth)
        };
        self.stats.cells_written += cells as u64;
        let cursor_changed = cursor != self.cursor || cells > 0 || self.full_redraw;
        if cursor_changed {
            match cursor {
                Some(spot) => {
                    self.encoder.move_to(spot.x, spot.y);
                    self.encoder.raw(if spot.visible {
                        seq::SHOW_CURSOR
                    } else {
                        seq::HIDE_CURSOR
                    });
                }
                None => self.encoder.raw(seq::HIDE_CURSOR),
            }
        }
        if sync {
            self.encoder.raw(seq::END_SYNC);
        }
        let bytes = self.encoder.take();
        self.write(&bytes);
        self.cursor = cursor;
        self.shown = next;
        self.full_redraw = false;
        self.dirty = false;
        self.last_frame = Some(now);
        self.stats.frames += 1;
        rustnative_core::perf::frame();
    }

    /// Ends the session: state flushed, the terminal given back.
    pub fn finish(&mut self) {
        if self.entered {
            let _ = self.app.flush_state();
            self.write(seq::RESTORE.as_bytes());
            self.entered = false;
        }
    }

    // ------------------------------------------------------------------
    // Input: host behaviour, then portable events
    // ------------------------------------------------------------------

    fn input(&mut self, event: InputEvent) {
        match event {
            InputEvent::Reply(reply) => self.reply(reply),
            InputEvent::Key(key) => self.key(key),
            InputEvent::Paste(text) => self.paste(&text),
            InputEvent::Mouse(mouse) => self.mouse(mouse),
            InputEvent::Focus(gained) => {
                let _ = gained;
                self.dirty = true;
            }
        }
    }

    fn reply(&mut self, reply: Reply) {
        if let Reply::Clipboard(text) = &reply {
            self.channel.clipboard_reply(text);
            if let Some((window, target)) = self.paste_pending.take() {
                self.insert_into(window, target, text);
            }
        }
        self.profile.accept(&reply);
        if matches!(reply, Reply::PrimaryAttributes(_)) && self.probe_until.is_some() {
            self.finish_probe();
        }
    }

    fn top(&self) -> Option<(WindowId, Option<NodeId>)> {
        let id = self.shown_window()?;
        Some((id, self.models.get(&id).and_then(TuiModel::focused)))
    }

    fn move_focus(&mut self, window: WindowId, next: Option<NodeId>) {
        let Some(model) = self.models.get_mut(&window) else { return };
        let previous = model.focused();
        if previous == next {
            return;
        }
        model.set_focused(next);
        if let Some(next) = next {
            model.reveal(next);
        }
        if let Some(previous) = previous {
            self.dispatch(window, Event::FocusLost { target: previous });
        }
        if let Some(next) = next {
            self.dispatch(window, Event::FocusGained { target: next });
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the host's key handling, in the order hosts apply it"
    )]
    fn key(&mut self, key: Key) {
        if key.kind == KeyKind::Release {
            if let Some((window, focused)) = self.top() {
                self.dispatch(
                    window,
                    Event::KeyUp { target: focused, key: key.code, modifiers: key.modifiers },
                );
            }
            return;
        }
        let Some((window, focused)) = self.top() else { return };
        // An open select's list takes its keys first.
        if let Some(target) = focused {
            if self.models.get(&window).is_some_and(|model| model.widget(target).open)
                && self.popup_key(window, target, &key)
            {
                return;
            }
        }
        let modifiers = key.modifiers;
        if key.code == KeyCode::Tab && !modifiers.ctrl && !modifiers.alt {
            let next = self
                .models
                .get(&window)
                .and_then(|model| model.focus_model().next(focused, modifiers.shift));
            self.move_focus(window, next);
            return;
        }
        if self.app.handle_shortcut(window, key.code, modifiers, focused) {
            self.dirty = true;
            return;
        }
        self.dispatch(window, Event::KeyDown { target: focused, key: key.code, modifiers });
        let handled = focused.is_some_and(|target| self.widget_key(window, target, &key));
        if !handled {
            if let Some(text) = &key.text {
                if !modifiers.ctrl && !modifiers.alt && !modifiers.meta && text != "\t" {
                    self.dispatch(window, Event::TextInput { target: focused, text: text.clone() });
                }
            }
            let ctrl_only = modifiers.ctrl && !modifiers.alt && !modifiers.shift && !modifiers.meta;
            match key.code {
                KeyCode::Character('c') if ctrl_only => self.close_top(),
                KeyCode::Character('z') if ctrl_only && self.config.suspend_key => self.suspend(),
                KeyCode::PageUp | KeyCode::PageDown => {
                    let sign = if key.code == KeyCode::PageDown { 1 } else { -1 };
                    if let Some(target) = focused {
                        let page = self.models.get(&window).and_then(|model| {
                            let scroller = model.scroller(target, true, sign)?;
                            Some((scroller, model.get(scroller)?.rect.height))
                        });
                        if let Some((scroller, height)) = page {
                            self.scroll_node(
                                window,
                                scroller,
                                0,
                                sign * (height - self.cell_height()).max(self.cell_height()),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        let focused = self.models.get(&window).and_then(TuiModel::focused);
        self.dispatch(window, Event::KeyUp { target: focused, key: key.code, modifiers });
    }

    fn cell_height(&self) -> i32 {
        i32::try_from(self.config.mapping.unit_height()).unwrap_or(16)
    }

    fn close_top(&mut self) {
        let Some(window) = self.shown_window() else { return };
        self.dispatch(window, Event::WindowCloseRequested { window });
        if window == WindowId::PRIMARY || self.stack.len() <= 1 {
            let _ = self.app.lifecycle(Lifecycle::Terminating);
            self.quit = true;
        } else {
            self.app.close_window(window);
            self.sync_windows();
        }
    }

    /// The focused widget's own keys; returns whether it used the key.
    #[allow(clippy::too_many_lines, reason = "one arm per widget")]
    fn widget_key(&mut self, window: WindowId, target: NodeId, key: &Key) -> bool {
        let Some(node) = self.models.get(&window).and_then(|model| model.get(target)).cloned()
        else {
            return false;
        };
        if !node.is_interactable() {
            return false;
        }
        let activate = matches!(key.code, KeyCode::Enter | KeyCode::Space)
            && !key.modifiers.ctrl
            && !key.modifiers.alt;
        let step = |code: KeyCode| match code {
            KeyCode::ArrowLeft | KeyCode::ArrowDown => Some(-1),
            KeyCode::ArrowRight | KeyCode::ArrowUp => Some(1),
            _ => None,
        };
        if node.is_text_field() {
            let multiline = node.kind != NodeKind::TextInput;
            let columns = usize::try_from(node.cells.width.max(1)).unwrap_or(1);
            let value =
                self.models.get(&window).map(|model| model.value(target)).unwrap_or_default();
            let Some(model) = self.models.get_mut(&window) else { return false };
            let mut widget = model.widget(target);
            let outcome = edit::key(&value, &mut widget.edit, key, multiline, columns, self.width);
            *model.widget_mut(target) = widget;
            if let Some(copied) = &outcome.copied {
                self.channel.copy(copied);
            }
            if outcome.paste {
                if self.profile.clipboard == ClipboardSupport::ReadWrite {
                    self.paste_pending = Some((window, target));
                    self.write(b"\x1b]52;c;?\x07");
                } else if let Some(text) = self.channel.local_clipboard() {
                    self.insert_into(window, target, &text);
                }
            }
            if let Some(next) = outcome.value {
                if let Some(model) = self.models.get_mut(&window) {
                    model.set_value(target, next.clone());
                }
                self.dispatch(window, Event::TextChanged { target, value: next });
            }
            self.dirty = true;
            return outcome.handled || outcome.paste;
        }
        match (&node.kind, &node.control) {
            (NodeKind::Button, _) if activate => {
                self.dispatch(window, Event::Click { target });
                true
            }
            (NodeKind::TabBar, _) => {
                let Some(tabs) = &node.tabs else { return false };
                let count = tabs.labels().len();
                let next = match key.code {
                    KeyCode::ArrowLeft => tabs.selected().checked_sub(1).or(count.checked_sub(1)),
                    KeyCode::ArrowRight => Some((tabs.selected() + 1) % count.max(1)),
                    KeyCode::Home => Some(0),
                    KeyCode::End => count.checked_sub(1),
                    _ => None,
                };
                match next {
                    Some(index) if index < count => {
                        self.dispatch(window, Event::TabSelected { target, index });
                        true
                    }
                    _ => false,
                }
            }
            (_, Some(Control::Checkbox { checked, .. })) if activate => {
                self.dispatch(window, Event::Toggled { target, on: !checked });
                true
            }
            (_, Some(Control::Toggle { on, .. })) if activate => {
                self.dispatch(window, Event::Toggled { target, on: !on });
                true
            }
            (_, Some(Control::Radio { .. })) if activate => {
                self.dispatch(window, Event::Toggled { target, on: true });
                true
            }
            (_, Some(Control::Link { .. })) if key.code == KeyCode::Enter => {
                self.dispatch(window, Event::Click { target });
                true
            }
            (
                _,
                Some(Control::Slider { value, min, max } | Control::Spinner { value, min, max }),
            ) => {
                let span = (max - min).max(1);
                let page = (span / 10).max(1);
                let next = match key.code {
                    KeyCode::Home => Some(*min),
                    KeyCode::End => Some(*max),
                    KeyCode::PageUp => Some(value + page),
                    KeyCode::PageDown => Some(value - page),
                    code => step(code).map(|delta| value + delta),
                };
                match next {
                    Some(next) => {
                        let next = next.clamp(*min, *max);
                        if next != *value {
                            self.dispatch(window, Event::ValueChanged { target, value: next });
                        }
                        true
                    }
                    None => false,
                }
            }
            (_, Some(Control::Select { options, selected })) => {
                let count = options.len();
                if activate || (key.code == KeyCode::ArrowDown && key.modifiers.alt) {
                    if let Some(model) = self.models.get_mut(&window) {
                        let widget = model.widget_mut(target);
                        widget.open = true;
                        widget.cursor = selected.or(Some(0));
                    }
                    self.dirty = true;
                    return true;
                }
                let next = match key.code {
                    KeyCode::ArrowUp => selected.map_or(Some(0), |index| index.checked_sub(1)),
                    KeyCode::ArrowDown => {
                        Some(selected.map_or(0, |index| (index + 1).min(count.saturating_sub(1))))
                    }
                    KeyCode::Home => Some(0),
                    KeyCode::End => count.checked_sub(1),
                    _ => return false,
                };
                if let Some(index) =
                    next.filter(|index| *index < count && Some(*index) != *selected)
                {
                    self.dispatch(window, Event::SelectionChanged { target, index: Some(index) });
                }
                true
            }
            (_, Some(Control::ListBox { items, selected })) => {
                let count = items.len();
                let rows = usize::try_from(node.cells.height.max(1)).unwrap_or(1);
                let cursor = self
                    .models
                    .get(&window)
                    .and_then(|model| model.widget(target).cursor)
                    .or(*selected);
                let moved = match key.code {
                    KeyCode::ArrowUp => Some(cursor.map_or(0, |c| c.saturating_sub(1))),
                    KeyCode::ArrowDown => {
                        Some(cursor.map_or(0, |c| (c + 1).min(count.saturating_sub(1))))
                    }
                    KeyCode::PageUp => Some(cursor.map_or(0, |c| c.saturating_sub(rows))),
                    KeyCode::PageDown => {
                        Some(cursor.map_or(0, |c| (c + rows).min(count.saturating_sub(1))))
                    }
                    KeyCode::Home => Some(0),
                    KeyCode::End => count.checked_sub(1),
                    KeyCode::Space | KeyCode::Enter => {
                        if let Some(index) = cursor.filter(|index| *index < count) {
                            if Some(index) != *selected {
                                self.dispatch(
                                    window,
                                    Event::SelectionChanged { target, index: Some(index) },
                                );
                            }
                        }
                        return true;
                    }
                    _ => return false,
                };
                if let Some(index) = moved.filter(|index| *index < count) {
                    if let Some(model) = self.models.get_mut(&window) {
                        model.widget_mut(target).cursor = Some(index);
                    }
                    // Moving the cursor selects, as a native single-choice
                    // list does.
                    if Some(index) != *selected {
                        self.dispatch(
                            window,
                            Event::SelectionChanged { target, index: Some(index) },
                        );
                    }
                }
                self.dirty = true;
                true
            }
            (_, Some(Control::DatePicker { date })) => {
                let Some(model) = self.models.get_mut(&window) else { return false };
                let widget = model.widget_mut(target);
                match key.code {
                    KeyCode::ArrowLeft => widget.segment = widget.segment.saturating_sub(1),
                    KeyCode::ArrowRight => widget.segment = (widget.segment + 1).min(2),
                    KeyCode::ArrowUp | KeyCode::ArrowDown => {
                        let delta: i32 = if key.code == KeyCode::ArrowUp { 1 } else { -1 };
                        let (mut year, mut month, mut day) =
                            (i32::from(date.year), i32::from(date.month), i32::from(date.day));
                        match widget.segment {
                            0 => year += delta,
                            1 => month = (month - 1 + delta).rem_euclid(12) + 1,
                            _ => day += delta,
                        }
                        let next = (1..=4)
                            .find_map(|back| {
                                let day = if widget.segment == 2 { day } else { day - back + 1 };
                                rustnative_core::CalendarDate::new(
                                    year.clamp(1, 9999),
                                    u8::try_from(month).unwrap_or(1),
                                    u8::try_from(day.clamp(1, 31)).unwrap_or(1),
                                )
                            })
                            .or_else(|| {
                                // Past the month's last day: wrap to the first.
                                rustnative_core::CalendarDate::new(
                                    year.clamp(1, 9999),
                                    u8::try_from(month).unwrap_or(1),
                                    1,
                                )
                            });
                        if let Some(next) = next.filter(|next| next != date) {
                            self.dispatch(window, Event::DateChanged { target, date: next });
                        }
                    }
                    _ => return false,
                }
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    fn popup_key(&mut self, window: WindowId, target: NodeId, key: &Key) -> bool {
        let Some(node) = self.models.get(&window).and_then(|model| model.get(target)).cloned()
        else {
            return false;
        };
        let Some(Control::Select { options, selected }) = &node.control else { return false };
        let count = options.len();
        let Some(model) = self.models.get_mut(&window) else { return false };
        let widget = model.widget_mut(target);
        let cursor = widget.cursor.or(*selected).unwrap_or(0);
        self.dirty = true;
        match key.code {
            KeyCode::Escape => widget.open = false,
            KeyCode::ArrowUp => widget.cursor = Some(cursor.saturating_sub(1)),
            KeyCode::ArrowDown => widget.cursor = Some((cursor + 1).min(count.saturating_sub(1))),
            KeyCode::Home => widget.cursor = Some(0),
            KeyCode::End => widget.cursor = count.checked_sub(1),
            KeyCode::PageUp => widget.cursor = Some(cursor.saturating_sub(8)),
            KeyCode::PageDown => widget.cursor = Some((cursor + 8).min(count.saturating_sub(1))),
            KeyCode::Enter | KeyCode::Space => {
                widget.open = false;
                if cursor < count && Some(cursor) != *selected {
                    self.dispatch(window, Event::SelectionChanged { target, index: Some(cursor) });
                }
            }
            KeyCode::Tab => {
                widget.open = false;
                return false;
            }
            _ => return true,
        }
        true
    }

    fn insert_into(&mut self, window: WindowId, target: NodeId, text: &str) {
        let Some(model) = self.models.get_mut(&window) else { return };
        let Some(node) = model.get(target).cloned() else { return };
        if !node.is_text_field() || !node.is_interactable() {
            return;
        }
        let value = model.value(target);
        let mut widget = model.widget(target);
        let next = edit::insert(&value, &mut widget.edit, text, node.kind != NodeKind::TextInput);
        *model.widget_mut(target) = widget;
        model.set_value(target, next.clone());
        self.dispatch(window, Event::TextChanged { target, value: next });
    }

    fn paste(&mut self, text: &str) {
        let Some((window, focused)) = self.top() else { return };
        let field = focused.filter(|target| {
            self.models
                .get(&window)
                .and_then(|model| model.get(*target))
                .is_some_and(|node| node.is_text_field())
        });
        match field {
            Some(target) => self.insert_into(window, target, text),
            None => {
                self.dispatch(window, Event::TextInput { target: focused, text: text.to_owned() })
            }
        }
    }

    fn scroll_node(&mut self, window: WindowId, node: NodeId, dx: i32, dy: i32) {
        let Some(model) = self.models.get_mut(&window) else { return };
        let Some(current) = model.get(node).map(|node| node.scroll) else { return };
        // A viewport change only: no render (`PLAN.md` 2.10); the next
        // frame places the content and reports a virtual list's range.
        if model.scroll_to(node, Point::new(current.x + dx, current.y + dy)) {
            self.dirty = true;
        }
    }

    fn pointer_event(
        &self,
        window: WindowId,
        target: NodeId,
        mouse: &Mouse,
        button: Option<PointerButton>,
    ) -> Option<PointerEvent> {
        let model = self.models.get(&window)?;
        let node = model.get(target)?;
        let point = self.config.mapping.unit_center(i32::from(mouse.column), i32::from(mouse.row));
        let local = Point::new(point.x - node.window_rect.x, point.y - node.window_rect.y);
        let mut event = PointerEvent::new(1, PointerKind::Mouse, local, self.now())
            .with_modifiers(mouse.modifiers);
        if let Some(button) = button {
            event = event.with_button(button).with_buttons(PointerButtons::none().with(button));
        }
        if let Some(list) = &node.draw_list {
            let region = list.hit_test(
                f32::from(i16::try_from(local.x).unwrap_or(i16::MAX)),
                f32::from(i16::try_from(local.y).unwrap_or(i16::MAX)),
            );
            event = event.with_region(region);
        }
        Some(event)
    }

    #[allow(clippy::too_many_lines, reason = "press, drag, release, motion, and wheel")]
    fn mouse(&mut self, mouse: Mouse) {
        let Some(window) = self.shown_window() else { return };
        let (x, y) = (i32::from(mouse.column), i32::from(mouse.row));
        let button = |button: MouseButton| match button {
            MouseButton::Left => Some(PointerButton::Primary),
            MouseButton::Right => Some(PointerButton::Secondary),
            MouseButton::Middle => Some(PointerButton::Middle),
            MouseButton::Other(8) => Some(PointerButton::Back),
            MouseButton::Other(9) => Some(PointerButton::Forward),
            MouseButton::Other(_) => None,
        };
        // An open select's list is hit first.
        if let Some(focused) = self.models.get(&window).and_then(TuiModel::focused) {
            if self.models.get(&window).is_some_and(|model| model.widget(focused).open) {
                if let MouseKind::Down(_) | MouseKind::Wheel(..) = mouse.kind {
                    if self.popup_mouse(window, focused, &mouse) {
                        return;
                    }
                }
            }
        }
        let hit = self.models.get(&window).and_then(|model| model.hit_test(x, y));
        match mouse.kind {
            MouseKind::Wheel(dx, dy) => {
                let Some(hit) = hit else { return };
                let vertical = dy != 0;
                let sign = i32::from(if vertical { dy } else { dx });
                let wheel_target = self.models.get(&window).and_then(|model| {
                    model
                        .ancestry(hit)
                        .into_iter()
                        .find(|id| model.get(*id).is_some_and(|node| node.input.wants_wheel()))
                });
                if let Some(target) = wheel_target {
                    let delta = WheelDelta::Lines {
                        x: i32::from(dx) * WheelDelta::DETENT,
                        y: -i32::from(dy) * WheelDelta::DETENT,
                    };
                    self.dispatch(window, Event::Wheel { target, delta });
                }
                // A list box scrolls its items; anything else its nearest
                // scrolling container.
                let list = self.models.get(&window).and_then(|model| {
                    let node = model.get(hit)?;
                    matches!(node.control, Some(Control::ListBox { .. })).then_some(hit)
                });
                if let Some(list) = list {
                    if let Some(model) = self.models.get_mut(&window) {
                        let widget = model.widget_mut(list);
                        widget.first = if sign > 0 {
                            widget.first + 3
                        } else {
                            widget.first.saturating_sub(3)
                        };
                    }
                    self.dirty = true;
                    return;
                }
                let scroller =
                    self.models.get(&window).and_then(|model| model.scroller(hit, vertical, sign));
                if let Some(scroller) = scroller {
                    let amount = WHEEL_LINES * sign;
                    let (dx, dy) = if vertical {
                        (0, amount * self.cell_height())
                    } else {
                        (amount * i32::try_from(self.config.mapping.unit_width()).unwrap_or(8), 0)
                    };
                    self.scroll_node(window, scroller, dx, dy);
                }
            }
            MouseKind::Down(pressed) => {
                let Some(target) = hit else {
                    self.close_popups(window);
                    return;
                };
                let interactable = self
                    .models
                    .get(&window)
                    .and_then(|model| model.get(target))
                    .is_some_and(|node| node.is_interactable());
                if !interactable {
                    return;
                }
                self.close_popups(window);
                // Focus moves to the nearest focusable node from the hit up.
                let focus = self.models.get(&window).and_then(|model| {
                    model.ancestry(target).into_iter().find(|id| {
                        model
                            .get(*id)
                            .is_some_and(|node| node.accessibility.is_focusable() && !node.disabled)
                    })
                });
                if pressed == MouseButton::Left {
                    if let Some(focus) = focus {
                        self.move_focus(window, Some(focus));
                    }
                }
                self.press = Some(Press { window, node: target, button: pressed });
                self.last_press_column = Some(x);
                if let Some(model) = self.models.get_mut(&window) {
                    model.set_pressed(Some(target));
                }
                if let Some(pointer) = self.pointer_target(window, target).and_then(|node| {
                    self.pointer_event(window, node, &mouse, button(pressed))
                        .map(|event| (node, event))
                }) {
                    self.dispatch(
                        window,
                        Event::PointerDown { target: pointer.0, pointer: pointer.1 },
                    );
                }
                if pressed == MouseButton::Left {
                    self.press_widget(window, focus.unwrap_or(target), x, y, false);
                }
                self.dirty = true;
            }
            MouseKind::Drag(held) => {
                let Some(press) = self.press else { return };
                let target = self.pointer_capture.map_or(press.node, |(node, _)| node);
                if let Some(pointer) = self.pointer_target(window, target).and_then(|node| {
                    self.pointer_event(window, node, &mouse, button(held))
                        .map(|event| (node, event))
                }) {
                    self.dispatch(
                        window,
                        Event::PointerMove { target: pointer.0, pointer: pointer.1 },
                    );
                }
                if held == MouseButton::Left {
                    let focus = self.models.get(&window).and_then(TuiModel::focused);
                    if let Some(focus) = focus {
                        self.press_widget(window, focus, x, y, true);
                    }
                }
            }
            MouseKind::Up(_) => {
                let Some(press) = self.press.take() else { return };
                if let Some(model) = self.models.get_mut(&press.window) {
                    model.set_pressed(None);
                }
                let target = self.pointer_capture.map_or(press.node, |(node, _)| node);
                if let Some(pointer) = self.pointer_target(window, target).and_then(|node| {
                    self.pointer_event(window, node, &mouse, button(press.button))
                        .map(|event| (node, event))
                }) {
                    self.dispatch(
                        window,
                        Event::PointerUp { target: pointer.0, pointer: pointer.1 },
                    );
                }
                self.pointer_capture = None;
                // A click: pressed and released over the same control.
                if press.button == MouseButton::Left && press.window == window {
                    let over = hit.is_some_and(|hit| {
                        hit == press.node
                            || self
                                .models
                                .get(&window)
                                .is_some_and(|model| model.ancestry(hit).contains(&press.node))
                    });
                    if over {
                        self.release_widget(window, press.node);
                    }
                }
                self.dirty = true;
            }
            MouseKind::Move => {
                let hover = hit.and_then(|hit| self.pointer_target(window, hit)).or(hit);
                let previous = self.models.get(&window).and_then(TuiModel::hovered);
                if hover != previous {
                    if let Some(model) = self.models.get_mut(&window) {
                        model.set_hovered(hover);
                    }
                    if let Some(previous) = previous.filter(|id| self.wants_pointer(window, *id)) {
                        self.dispatch(window, Event::PointerLeave { target: previous });
                    }
                    if let Some(next) = hover.filter(|id| self.wants_pointer(window, *id)) {
                        self.dispatch(window, Event::PointerEnter { target: next });
                    }
                    self.dirty = true;
                }
                if let Some(target) = hit.and_then(|hit| self.pointer_target(window, hit)) {
                    if let Some(event) = self.pointer_event(window, target, &mouse, None) {
                        self.dispatch(window, Event::PointerMove { target, pointer: event });
                    }
                }
            }
        }
    }

    fn wants_pointer(&self, window: WindowId, id: NodeId) -> bool {
        self.models
            .get(&window)
            .and_then(|model| model.get(id))
            .is_some_and(|node| node.input.wants_pointer())
    }

    /// The nearest node from `id` up with pointer interest.
    fn pointer_target(&self, window: WindowId, id: NodeId) -> Option<NodeId> {
        let model = self.models.get(&window)?;
        model
            .ancestry(id)
            .into_iter()
            .find(|candidate| model.get(*candidate).is_some_and(|node| node.input.wants_pointer()))
    }

    fn close_popups(&mut self, window: WindowId) {
        if let Some(model) = self.models.get_mut(&window) {
            if let Some(focused) = model.focused() {
                if model.widget(focused).open {
                    model.widget_mut(focused).open = false;
                    self.dirty = true;
                }
            }
        }
    }

    fn popup_mouse(&mut self, window: WindowId, select: NodeId, mouse: &Mouse) -> bool {
        let Some(node) = self.models.get(&window).and_then(|model| model.get(select)).cloned()
        else {
            return false;
        };
        let Some(Control::Select { options, selected }) = &node.control else { return false };
        let frame = paint::popup_frame(&node, options, self.shown.bounds(), self.width);
        let (x, y) = (i32::from(mouse.column), i32::from(mouse.row));
        if !frame.contains(x, y) {
            self.close_popups(window);
            // A press outside closes the list and is not delivered further
            // when it lands on the select itself.
            return node.visible_cells.contains(x, y);
        }
        let rows = usize::try_from(frame.height - 2).unwrap_or(0);
        let Some(model) = self.models.get_mut(&window) else { return true };
        let widget = model.widget_mut(select);
        let cursor = widget.cursor.or(*selected).unwrap_or(0);
        let first = paint::popup_first(options.len(), cursor, rows);
        match mouse.kind {
            MouseKind::Wheel(_, dy) => {
                widget.cursor = Some(if dy > 0 {
                    (cursor + 1).min(options.len().saturating_sub(1))
                } else {
                    cursor.saturating_sub(1)
                });
            }
            _ => {
                if y > frame.y && y < frame.bottom() - 1 {
                    let index = first + usize::try_from(y - frame.y - 1).unwrap_or(0);
                    widget.open = false;
                    if index < options.len() && Some(index) != *selected {
                        self.dispatch(
                            window,
                            Event::SelectionChanged { target: select, index: Some(index) },
                        );
                    }
                }
            }
        }
        self.dirty = true;
        true
    }

    /// What a press (or, with `drag`, a drag) at cell (`x`, `y`) does to
    /// widget `target`.
    fn press_widget(&mut self, window: WindowId, target: NodeId, x: i32, y: i32, drag: bool) {
        let Some(node) = self.models.get(&window).and_then(|model| model.get(target)).cloned()
        else {
            return;
        };
        if !node.is_interactable() {
            return;
        }
        let column = x - node.cells.x;
        let row = y - node.cells.y;
        if node.is_text_field() {
            let multiline = node.kind != NodeKind::TextInput;
            let value =
                self.models.get(&window).map(|model| model.value(target)).unwrap_or_default();
            let columns = usize::try_from(node.cells.width.max(1)).unwrap_or(1);
            let Some(model) = self.models.get_mut(&window) else { return };
            let widget = model.widget_mut(target);
            let lines = edit::visual_lines(&value, columns, self.width, multiline);
            let (line, column) = if multiline {
                (
                    widget.edit.scroll + usize::try_from(row.max(0)).unwrap_or(0),
                    usize::try_from(column.max(0)).unwrap_or(0),
                )
            } else {
                (0, widget.edit.scroll + usize::try_from(column.max(0)).unwrap_or(0))
            };
            let offset = edit::offset_at(&value, &lines, column, line, self.width);
            widget.edit.caret = offset;
            if !drag {
                widget.edit.anchor = offset;
            }
            self.dirty = true;
            return;
        }
        match &node.control {
            Some(Control::Slider { value, min, max }) => {
                let next = paint::slider_value_at(&node, *min, *max, column);
                if next != *value {
                    self.dispatch(window, Event::ValueChanged { target, value: next });
                }
            }
            Some(Control::ListBox { items, selected }) if !drag => {
                let first = self.models.get(&window).map_or(0, |model| model.widget(target).first);
                let index = first + usize::try_from(row.max(0)).unwrap_or(0);
                if index < items.len() {
                    if let Some(model) = self.models.get_mut(&window) {
                        model.widget_mut(target).cursor = Some(index);
                    }
                    if Some(index) != *selected {
                        self.dispatch(
                            window,
                            Event::SelectionChanged { target, index: Some(index) },
                        );
                    }
                }
            }
            Some(Control::DatePicker { .. }) if !drag => {
                let segment = match column {
                    ..=4 => 0,
                    5..=7 => 1,
                    _ => 2,
                };
                if let Some(model) = self.models.get_mut(&window) {
                    model.widget_mut(target).segment = segment;
                }
                self.dirty = true;
            }
            _ => {}
        }
        if node.kind == NodeKind::TabBar && !drag {
            if let Some(tabs) = &node.tabs {
                let spans = paint::tab_spans(tabs.labels(), self.width);
                if let Some(index) =
                    spans.iter().position(|(start, end)| column >= *start && column < *end)
                {
                    if index != tabs.selected() {
                        self.dispatch(window, Event::TabSelected { target, index });
                    }
                }
            }
        }
    }

    /// What a click (press and release over it) does to `target`.
    fn release_widget(&mut self, window: WindowId, target: NodeId) {
        let Some(node) = self.models.get(&window).and_then(|model| {
            model
                .ancestry(target)
                .into_iter()
                .filter_map(|id| model.get(id))
                .find(|node| {
                    node.is_interactable()
                        && (node.kind == NodeKind::Button || node.control.is_some())
                })
                .cloned()
        }) else {
            return;
        };
        let target = node.id;
        match (&node.kind, &node.control) {
            (NodeKind::Button, _) => self.dispatch(window, Event::Click { target }),
            (_, Some(Control::Checkbox { checked, .. })) => {
                self.dispatch(window, Event::Toggled { target, on: !checked })
            }
            (_, Some(Control::Toggle { on, .. })) => {
                self.dispatch(window, Event::Toggled { target, on: !on })
            }
            (_, Some(Control::Radio { .. })) => {
                self.dispatch(window, Event::Toggled { target, on: true })
            }
            (_, Some(Control::Link { .. })) => self.dispatch(window, Event::Click { target }),
            (_, Some(Control::Select { selected, .. })) => {
                if let Some(model) = self.models.get_mut(&window) {
                    let widget = model.widget_mut(target);
                    widget.open = true;
                    widget.cursor = selected.or(Some(0));
                }
                self.dirty = true;
            }
            (_, Some(Control::Spinner { value, min, max })) => {
                // The left half steps down, the right half up.
                let half = node.cells.x + node.cells.width / 2;
                let pressed_left = self.last_press_column(window).is_some_and(|x| x < half);
                let next = if pressed_left { value - 1 } else { value + 1 };
                let next = next.clamp(*min, *max);
                if next != *value {
                    self.dispatch(window, Event::ValueChanged { target, value: next });
                }
            }
            _ => {}
        }
    }

    fn last_press_column(&self, _window: WindowId) -> Option<i32> {
        self.last_press_column
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a component panicked with a non-string payload".to_owned())
}

fn apply_frame(
    overrides: &mut HashMap<(NodeId, AnimatedProperty), AnimatedValue>,
    frame: &rustnative_core::Frame,
) {
    match frame.value {
        Some(value) => {
            overrides.insert((frame.node, frame.property), value);
        }
        None => {
            overrides.remove(&(frame.node, frame.property));
        }
    }
}

/// The value a property has in the rendered tree — what an animation
/// starts from when it names no start.
fn current_value(
    model: &TuiModel,
    app: &Application,
    node: NodeId,
    property: AnimatedProperty,
) -> Option<AnimatedValue> {
    let node = model.get(node)?;
    let theme = app.theme();
    let resolved =
        theme.resolve(node.kind, rustnative_core::ControlState::Normal, &node.style_override);
    match property {
        AnimatedProperty::Opacity => Some(AnimatedValue::Scalar(Scalar::new(node.opacity))),
        AnimatedProperty::Background => {
            resolved.properties().background_override().map(AnimatedValue::Color)
        }
        AnimatedProperty::Foreground => {
            resolved.properties().foreground_override().map(AnimatedValue::Color)
        }
        _ => None,
    }
}

/// Each transitioned property a cell can show (opacity and colours), with
/// its rendered value and transition.
fn transitioned_values(
    model: &TuiModel,
    theme: &Theme,
) -> HashMap<(NodeId, AnimatedProperty), (AnimatedValue, rustnative_core::Transition)> {
    let mut out = HashMap::new();
    for node in model.nodes() {
        for transition in &node.transitions {
            let resolved = theme.resolve(
                node.kind,
                rustnative_core::ControlState::Normal,
                &node.style_override,
            );
            let value = match transition.property {
                AnimatedProperty::Opacity => Some(AnimatedValue::Scalar(Scalar::new(node.opacity))),
                AnimatedProperty::Background => {
                    resolved.properties().background_override().map(AnimatedValue::Color)
                }
                AnimatedProperty::Foreground => {
                    resolved.properties().foreground_override().map(AnimatedValue::Color)
                }
                // Position and size cannot move by less than a cell, so
                // they change in one step.
                _ => None,
            };
            if let Some(value) = value {
                out.insert((node.id, transition.property), (value, transition.transition));
            }
        }
    }
    out
}
