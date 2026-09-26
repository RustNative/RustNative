//! Layout conformance (`PLAN.md` Milestone 41): the reference screen at
//! text scales 1.0, 1.5, and 2.0, plain and pseudo-localized (+40 %,
//! accented), left to right and mirrored — with no clipped text, no
//! overlapping siblings, no interactive target under 24×24 or outside its
//! container, and every focusable control reachable by Tab.
//!
//! The headless backend's metrics are deterministic, so this runs the same
//! everywhere; `rustnative-windows` runs the same screen with the system's
//! own font metrics (`native::guarantees_integration`).

use std::collections::BTreeSet;

use rustnative_conformance::reference::{ReferenceScreen, Variant};
use rustnative_core::{Component, IntrinsicMeasurer, NodeId, NodeKind, Size, Window};
use rustnative_headless::{HeadlessApp, HeadlessMeasurer, RealizedNode};

fn intersects(a: &RealizedNode, b: &RealizedNode) -> bool {
    let (a, b) = (a.rect, b.rect);
    a.width > 0
        && a.height > 0
        && b.width > 0
        && b.height > 0
        && a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}

fn check(scale: f32, variant: Variant) -> Vec<String> {
    let context = format!("scale {scale}, {variant:?}");
    let measurer = HeadlessMeasurer::with_text_scale(scale);
    let mut app = HeadlessApp::launch(Window::new("reference", Size::new(480, 900)), move || {
        ReferenceScreen::new(variant)
    })
    .with_measurer(measurer);
    let mut problems = Vec::new();
    let nodes: Vec<RealizedNode> = app.realized().nodes().cloned().collect();
    let by_id = |id: NodeId| nodes.iter().find(|node| node.id == id);

    for node in &nodes {
        let name = node.key.clone().unwrap_or_default();
        // No clipped text.
        if let Some(text) = &node.text {
            match node.kind {
                // Labels wrap; a tab strip too wide for one row takes more
                // rows (the host's own behaviour): both must be as tall as
                // their text at their width.
                NodeKind::Label | NodeKind::TabBar => {
                    let needed = measurer.measure(node.kind, Some(text), Some(node.rect.width));
                    let needed_height = i32::try_from(needed.height).unwrap_or(i32::MAX);
                    if needed_height > node.rect.height {
                        problems.push(format!(
                            "{context}: `{name}` clips: needs {needed_height}px, has {}",
                            node.rect.height
                        ));
                    }
                }
                NodeKind::Button => {
                    let needed = measurer.measure(node.kind, Some(text), None);
                    let (width, height) = (
                        i32::try_from(needed.width).unwrap_or(i32::MAX),
                        i32::try_from(needed.height).unwrap_or(i32::MAX),
                    );
                    if width > node.rect.width || height > node.rect.height {
                        problems.push(format!(
                            "{context}: `{name}` clips: needs {width}×{height}, has {}×{}",
                            node.rect.width, node.rect.height
                        ));
                    }
                }
                _ => {}
            }
        }
        // Interactive targets are big enough and inside their container.
        if matches!(node.kind, NodeKind::Button | NodeKind::TextInput | NodeKind::TabBar) {
            if node.rect.width < 24 || node.rect.height < 24 {
                problems.push(format!(
                    "{context}: `{name}` is {}×{}, under 24×24",
                    node.rect.width, node.rect.height
                ));
            }
            if let Some(parent) = node.parent.and_then(by_id) {
                let fits = node.rect.x >= 0
                    && node.rect.y >= 0
                    && node.rect.x + node.rect.width <= parent.rect.width
                    && node.rect.y + node.rect.height <= parent.rect.height;
                if !fits {
                    problems.push(format!("{context}: `{name}` lies outside its container"));
                }
            }
        }
        // Siblings do not overlap.
        let children: Vec<&RealizedNode> =
            node.children.iter().filter_map(|id| by_id(*id)).collect();
        for (index, first) in children.iter().enumerate() {
            for second in &children[index + 1..] {
                if intersects(first, second) {
                    problems.push(format!(
                        "{context}: `{}` and `{}` overlap",
                        first.key.clone().unwrap_or_default(),
                        second.key.clone().unwrap_or_default()
                    ));
                }
            }
        }
    }

    // Every focusable control is reachable by Tab.
    let focusable: BTreeSet<NodeId> = nodes
        .iter()
        .filter(|node| node.accessibility.is_focusable() && !node.disabled && !node.hidden)
        .map(|node| node.id)
        .collect();
    let mut reached = BTreeSet::new();
    for _ in 0..focusable.len() * 2 {
        app.tab();
        if let Some(node) = app.realized().nodes().find(|node| node.focused) {
            reached.insert(node.id);
        }
    }
    if reached != focusable {
        problems.push(format!(
            "{context}: Tab reaches {} of {} focusable controls",
            reached.len(),
            focusable.len()
        ));
    }
    problems
}

#[test]
fn the_reference_screen_holds_at_every_scale_language_and_direction() {
    let mut problems = Vec::new();
    for scale in [1.0, 1.5, 2.0] {
        for pseudo in [false, true] {
            for right_to_left in [false, true] {
                problems.extend(check(scale, Variant { pseudo, right_to_left }));
            }
        }
    }
    assert!(problems.is_empty(), "layout conformance:\n  {}", problems.join("\n  "));
}
