//! The in-application overlay: for a host an external client cannot
//! attach to, the inspector's views drawn over the application itself, as
//! an ordinary [`DrawList`] — the same path a canvas draws through, so no
//! backend needs a second renderer.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::TraceEntry;
use super::trace::TraceKind;
use crate::graphics::{DrawList, Paint, RectF, Vec2};
use crate::identity::NodeId;
use crate::layout::Rect;
use crate::style::Color;

/// What the overlay draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    /// Every node's rectangle, labelled with its key.
    Layout,
    /// The targets of the most recent events, newest brightest.
    Events,
    /// How long each recent event or task delivery took, as bars.
    FrameCost,
}

/// How many recent events and costs the overlay shows.
const RECENT: usize = 24;

fn rect_f(rect: Rect) -> RectF {
    #[allow(
        clippy::cast_precision_loss,
        reason = "window coordinates are far inside f32's exact range"
    )]
    RectF::new(rect.x as f32, rect.y as f32, rect.width as f32, rect.height as f32)
}

/// The overlay for `mode`, over nodes laid out at `rects` (window
/// coordinates), given the recent `trace`.
pub(crate) fn draw(
    mode: OverlayMode,
    rects: &HashMap<NodeId, Rect>,
    trace: &[&TraceEntry],
    width: u32,
) -> DrawList {
    let mut list = DrawList::new();
    match mode {
        OverlayMode::Layout => {
            let mut nodes: Vec<_> = rects.iter().collect();
            nodes.sort_by_key(|(id, _)| id.get());
            for (id, rect) in nodes {
                let paint = Paint::color(Color::rgba(0, 120, 215, 200));
                list = list.stroke_rect(rect_f(*rect), paint);
                if let Some(key) = id.local_key() {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "window coordinates fit f32 exactly"
                    )]
                    let origin = Vec2::new(rect.x as f32 + 2.0, rect.y as f32 + 1.0);
                    list = list.text(origin, key, 10.0, Color::rgba(0, 90, 170, 255));
                }
            }
        }
        OverlayMode::Events => {
            let targets: Vec<NodeId> = trace
                .iter()
                .rev()
                .filter_map(|entry| match &entry.kind {
                    TraceKind::Event { event, .. } => event_target(event, rects),
                    _ => None,
                })
                .take(RECENT)
                .collect();
            for (age, target) in targets.iter().enumerate() {
                if let Some(rect) = rects.get(target) {
                    let alpha = u8::try_from(255usize.saturating_sub(age * 10)).unwrap_or(0);
                    list = list.stroke_rect(
                        rect_f(*rect),
                        Paint::color(Color::rgba(232, 17, 35, alpha)).stroke_width(2.0),
                    );
                }
            }
        }
        OverlayMode::FrameCost => {
            let costs: Vec<u64> =
                trace.iter().rev().take(RECENT).map(|entry| entry.micros).collect();
            // One bar per entry, newest at the right; a bar's height is
            // its cost against a 16.7 ms frame, which a line marks.
            let right = f32::from(u16::try_from(width).unwrap_or(u16::MAX));
            let frame = Paint::color(Color::rgba(0, 0, 0, 160));
            list = list.stroke_line(Vec2::new(right - 250.0, 40.0), Vec2::new(right, 40.0), frame);
            for (index, micros) in costs.iter().enumerate() {
                #[allow(clippy::cast_precision_loss, reason = "a bar's height needs no precision")]
                let height = (*micros as f32 / 16_700.0 * 40.0).min(80.0);
                #[allow(clippy::cast_precision_loss, reason = "fewer than 25 bars")]
                let x = right - 10.0 * (index as f32 + 1.0);
                let color = if *micros > 16_700 {
                    Color::rgb(232, 17, 35)
                } else {
                    Color::rgb(16, 124, 16)
                };
                list =
                    list.fill_rect(RectF::new(x, 80.0 - height, 8.0, height), Paint::color(color));
            }
        }
    }
    list
}

/// The node an event's debug text names, found among `rects`.
fn event_target(event: &str, rects: &HashMap<NodeId, Rect>) -> Option<NodeId> {
    // `target: NodeId(<n>)` — the only form an event's target prints in.
    let start = event.find("NodeId(")? + "NodeId(".len();
    let end = start + event[start..].find(')')?;
    let id: u128 = event[start..end].parse().ok()?;
    rects.keys().copied().find(|node| node.get() == id)
}
