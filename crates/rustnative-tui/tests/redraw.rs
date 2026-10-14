//! Scheduling and redraw (`docs/superpowers/plans/2026-10-13-terminal-
//! milestone-38.md`, Phase 4): a change writes only its cells, a resize
//! redraws once, frames coalesce under load, and animations run through the
//! same `Timeline` — or not at all under reduced motion.

mod common;

use std::time::Duration;

use rustnative_core::{
    AnimatedProperty, Component, Control, Event, KeyCode, KeyModifiers, MotionPreference, Node,
    NodeId, Transition,
};
use rustnative_tui::TuiHarness;
use rustnative_tui::grid::Attrs;

/// A counter and a field, in a full-size screen of other text.
struct Busy {
    count: u32,
    field: String,
    faded: bool,
}

impl Component for Busy {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { count: 0, field: String::new(), faded: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let filler = (0..12).map(|row| {
            Node::label(
                format!("filler-{row}"),
                format!("line {row} of unchanging text, wide enough to fill the row"),
            )
        });
        let mut children: Vec<Node> = vec![
            Node::label("count", format!("Count: {}", self.count)),
            Node::button("more", "More"),
            Node::text_input("field", self.field.clone()),
            Node::label("fade", "Fading text")
                .with_opacity(if self.faded { 0.0 } else { 1.0 })
                .with_transition(
                    AnimatedProperty::Opacity,
                    Transition::new(Duration::from_millis(400)),
                ),
            Node::control("spinner", Control::Progress { percent: None }),
        ];
        children.extend(filler);
        common::dense("root", children)
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("more") => self.count += 1,
            Event::TextChanged { value, .. } => self.field = value,
            Event::KeyDown { key: KeyCode::Function(2), .. } => self.faded = !self.faded,
            _ => {}
        }
    }
}

fn launch() -> TuiHarness {
    TuiHarness::launch(common::window("busy"), 80, 24, || Busy::new(()))
}

#[test]
fn a_change_writes_only_its_cells() {
    let mut terminal = launch();
    terminal.session_mut().set_motion_preference(MotionPreference::Reduced);
    terminal.settle();
    terminal.press_tab();
    let before = terminal.stats();
    let _ = terminal.take_written();
    terminal.press_enter();
    let after = terminal.stats();
    let written = terminal.take_written();
    assert!(terminal.text().contains("Count: 1"));
    assert_eq!(after.cells_written - before.cells_written, 1, "only the digit changed");
    assert!(
        written.len() < 40,
        "a one-cell change is a few bytes, not a screen: {} bytes {:?}",
        written.len(),
        String::from_utf8_lossy(&written)
    );
    assert_eq!(after.full_redraws, before.full_redraws, "no full redraw");
}

#[test]
fn typing_writes_a_few_bytes_a_keystroke() {
    let mut terminal = launch();
    terminal.session_mut().set_motion_preference(MotionPreference::Reduced);
    terminal.click_key("field");
    let _ = terminal.take_written();
    let text = "the quick brown fox";
    terminal.type_text(text);
    let written = terminal.take_written().len();
    let per_key = written / text.len();
    assert!(per_key <= 32, "{per_key} bytes a keystroke");
}

#[test]
fn a_resize_redraws_once() {
    let mut terminal = launch();
    terminal.session_mut().set_motion_preference(MotionPreference::Reduced);
    terminal.settle();
    let before = terminal.stats();
    terminal.resize(100, 30);
    let after = terminal.stats();
    assert_eq!(after.full_redraws - before.full_redraws, 1, "one full redraw");
    assert_eq!(after.frames - before.frames, 1, "in one frame");
    assert_eq!(terminal.screen().columns(), 100);
    assert!(terminal.text().contains("Count: 0"));
}

#[test]
fn frames_coalesce_under_load() {
    let mut terminal = launch();
    terminal.session_mut().set_motion_preference(MotionPreference::Reduced);
    terminal.click_key("field");
    let before = terminal.stats().frames;
    // Fifty keystrokes arrive within one frame interval: the loop handles
    // each, and draws once when the interval is up.
    for character in "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwx".chars() {
        terminal.terminal().send(character.to_string().as_bytes());
        terminal.session_mut().step(Some(Duration::ZERO));
    }
    assert_eq!(terminal.stats().frames, before, "no frame before the interval is up");
    terminal.advance(Duration::from_millis(40));
    assert_eq!(terminal.stats().frames, before + 1, "one frame for all fifty");
    assert!(
        terminal.text().contains("abcdefghijklmnopqrstuvwxyzabcdefghijkl"),
        "{}",
        terminal.text()
    );
}

fn fade_attrs(terminal: &TuiHarness) -> Option<Attrs> {
    let node = terminal.node("fade")?;
    let row = u16::try_from(node.cells.y).ok()?;
    let cell = terminal.screen().row(row).first().cloned()?;
    (cell.text != " ").then_some(cell.style.attrs)
}

#[test]
fn a_transition_runs_on_the_timeline_through_faint_to_gone() {
    let mut terminal = launch();
    assert_eq!(fade_attrs(&terminal), Some(Attrs::NONE));
    terminal.press(KeyCode::Function(2), KeyModifiers::default());
    terminal.advance(Duration::from_millis(100));
    assert_eq!(
        fade_attrs(&terminal),
        Some(Attrs::NONE),
        "a quarter of the way: still above one half"
    );
    terminal.advance(Duration::from_millis(150));
    assert_eq!(fade_attrs(&terminal), Some(Attrs::DIM), "past half way: faint");
    terminal.advance(Duration::from_millis(250));
    assert_eq!(fade_attrs(&terminal), None, "at zero: not drawn");
}

#[test]
fn reduced_motion_skips_the_transition_and_stills_progress() {
    let mut terminal = launch();
    terminal.session_mut().set_motion_preference(MotionPreference::Reduced);
    terminal.settle();
    terminal.press(KeyCode::Function(2), KeyModifiers::default());
    terminal.advance(Duration::from_millis(20));
    assert_eq!(fade_attrs(&terminal), None, "gone at once");
    // The instant transition hands its property back on the next tick.
    terminal.advance(Duration::from_millis(100));
    let frames = terminal.stats().frames;
    terminal.advance(Duration::from_secs(1));
    assert_eq!(terminal.stats().frames, frames, "nothing animates, so nothing is drawn");
}

#[test]
fn indeterminate_progress_moves_at_the_frame_rate() {
    let mut terminal = launch();
    let frames = terminal.stats().frames;
    let first = terminal.text();
    terminal.advance(Duration::from_millis(500));
    assert!(terminal.stats().frames > frames, "the busy indicator is redrawn");
    assert!(terminal.stats().frames <= frames + 16, "at a terminal-sane rate");
    assert_ne!(terminal.text(), first);
}
