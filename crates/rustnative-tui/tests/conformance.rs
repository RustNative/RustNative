//! The terminal backend under the shared suites (`PLAN.md` Milestone 41):
//! the guarantee suites, run through [`ConformanceHost`] on the synthetic
//! terminal, and layout conformance on the reference screen — plain,
//! pseudo-localized, mirrored, and in wide characters — at several
//! terminal sizes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use rustnative_conformance::host::{ConformanceHost, Driver};
use rustnative_conformance::reference::{ReferenceScreen, Variant};
use rustnative_core::{Component, IntrinsicMeasurer, Node, NodeKind, Window};
use rustnative_drawlist::GridRect;
use rustnative_tui::{TuiHarness, TuiMeasurer};

struct TuiHost;

struct TuiDriver(TuiHarness);

impl Driver for TuiDriver {
    fn click(&mut self, key: &str) {
        self.0.click_key(key);
    }

    fn type_text(&mut self, key: &str, text: &str) {
        if self.0.focused_key().as_deref() != Some(key) {
            self.0.click_key(key);
        }
        self.0.type_text(text);
    }

    fn scroll(&mut self, key: &str, dy: i32) {
        let node = self.0.node(key).unwrap_or_else(|| panic!("no `{key}` to scroll"));
        let cells = node.visible_cells;
        let (column, row) = (
            u16::try_from(cells.x + cells.width / 2).unwrap_or(0),
            u16::try_from(cells.y + cells.height / 2).unwrap_or(0),
        );
        // A notch scrolls three rows (48 logical pixels).
        let notches = (dy.abs() / 48).max(1);
        for _ in 0..notches {
            self.0.wheel(column, row, dy > 0);
        }
    }

    fn advance(&mut self, duration: Duration) {
        self.0.advance(duration);
    }

    fn realized_objects(&self) -> usize {
        self.0.model().nodes().count()
    }
}

impl ConformanceHost for TuiHost {
    fn name(&self) -> &'static str {
        "terminal"
    }

    fn run<C, F>(&mut self, window: Window, root: F, script: &mut dyn FnMut(&mut dyn Driver))
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        let mut driver = TuiDriver(TuiHarness::launch(window, 100, 60, root));
        script(&mut driver);
    }
}

#[test]
fn the_guarantee_suites_hold_on_the_synthetic_terminal() {
    rustnative_conformance::suites::all(&mut TuiHost);
}

/// What layout conformance asks of a realized screen: no text clipped, no
/// siblings overlapping in cells, every focusable control reachable by Tab
/// and at least a cell in size, and nothing outside its parent.
fn problems(terminal: &TuiHarness, context: &str) -> Vec<String> {
    let model = terminal.model();
    let measurer = TuiMeasurer::new(terminal.session().mapping(), terminal.session().width());
    let mut problems = Vec::new();
    for node in model.nodes() {
        let name = node.key.clone().unwrap_or_default();
        if node.hidden {
            continue;
        }
        if let (Some(text), NodeKind::Label | NodeKind::TabBar) = (&node.text, node.kind) {
            let needed = measurer.measure(node.kind, Some(text), Some(node.rect.width));
            if i32::try_from(needed.height).unwrap_or(i32::MAX) > node.rect.height {
                problems.push(format!("{context}: `{name}` clips its text"));
            }
        }
        if node.accessibility.is_focusable() && !node.disabled && node.cells.is_empty() {
            problems.push(format!("{context}: `{name}` takes no cell"));
        }
        // What a node draws stays inside what its parent shows (a row too
        // narrow for its children clips them, as the portable layout says).
        if let Some(parent) = node.parent.and_then(|parent| model.get(parent)) {
            let visible = node.visible_cells;
            if !visible.is_empty() && visible.intersect(&parent.visible_cells) != visible {
                problems.push(format!(
                    "{context}: `{name}` draws at {visible:?}, outside its parent's {:?}",
                    parent.visible_cells
                ));
            }
        }
        let children: Vec<GridRect> = node
            .children
            .iter()
            .filter_map(|child| model.get(*child))
            .filter(|c| !c.hidden)
            .map(|c| c.cells)
            .collect();
        for (index, a) in children.iter().enumerate() {
            for b in &children[index + 1..] {
                if !a.intersect(b).is_empty() {
                    problems.push(format!("{context}: children of `{name}` overlap: {a:?} {b:?}"));
                }
            }
        }
    }
    let reachable: BTreeSet<_> = model.focus_model().order().iter().copied().collect();
    for node in model.nodes() {
        if node.accessibility.is_focusable()
            && !node.disabled
            && !node.hidden
            && !reachable.contains(&node.id)
        {
            problems.push(format!("{context}: `{:?}` cannot be reached by Tab", node.key));
        }
    }
    problems
}

#[test]
fn the_reference_screen_lays_out_cleanly_plain_pseudo_localized_and_mirrored() {
    let mut all = Vec::new();
    for (columns, rows) in [(80, 40), (60, 50), (120, 40)] {
        for variant in [
            Variant { pseudo: false, right_to_left: false },
            Variant { pseudo: true, right_to_left: false },
            Variant { pseudo: false, right_to_left: true },
            Variant { pseudo: true, right_to_left: true },
        ] {
            let terminal =
                TuiHarness::launch(common::window("reference"), columns, rows, move || {
                    ReferenceScreen::new(variant)
                });
            all.extend(problems(&terminal, &format!("{columns}x{rows} {variant:?}")));
        }
    }
    assert!(all.is_empty(), "{}", all.join("\n"));
}

/// The same checks over text in wide characters, combining marks, and
/// emoji, in a narrow terminal where it has to wrap.
#[test]
fn wide_characters_wrap_within_their_cells() {
    struct Wide;
    impl Component for Wide {
        type Props = ();
        type Message = ();
        fn new((): ()) -> Self {
            Self
        }
        fn props(&self) -> &() {
            &()
        }
        fn set_props(&mut self, (): ()) {}
        fn view(&self) -> Node {
            common::dense(
                "root",
                [
                    Node::label("cjk", "設定を保存すると、変更はすぐに反映されます。"),
                    Node::label("korean", "변경 사항은 저장할 때 적용됩니다"),
                    Node::label("marks", "ạ̈ccệnts and e\u{301}moji 👩‍👩‍👧 🇯🇵 together"),
                    Node::row(
                        "actions",
                        [Node::button("save", "保存"), Node::button("cancel", "取消")],
                    ),
                    Node::text_input("field", "日本語の入力"),
                ],
            )
        }
        fn update(&mut self, _event: rustnative_core::Event) {}
    }
    for columns in [20_u16, 13, 9] {
        let terminal = TuiHarness::launch(common::window("wide"), columns, 30, || Wide);
        let found = problems(&terminal, &format!("{columns} columns"));
        assert!(found.is_empty(), "{}", found.join("\n"));
        // No wide character is cut at the screen's edge.
        let screen = terminal.screen();
        for row in 0..screen.rows() {
            let last = screen.row(row).last().unwrap();
            assert!(last.width != 2, "a wide character straddles the edge on row {row}");
        }
        let text = terminal.text();
        assert!(text.contains("設定"), "{text}");
    }
}
