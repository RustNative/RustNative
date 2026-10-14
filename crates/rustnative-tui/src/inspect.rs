//! The inspection protocol, answered over its side channel (`PLAN.md`
//! Milestone 38: "the inspection protocol answered over a side channel
//! rather than a second screen, since the terminal is already the output
//! surface"; Milestone 44).
//!
//! The protocol's transport is already a side channel — line-delimited JSON
//! over a loopback TCP socket (`rustnative_core::inspect::server`), started
//! by `RUSTNATIVE_INSPECT=1` — so a terminal application is inspected from
//! another terminal with `rustnative inspect`, exactly as a desktop one is.
//! What this module adds is the backend's half: its realized objects (each
//! node's cells), their rectangles, lifetimes, capabilities, and tables.
//! The overlay, when an inspector turns it on, is drawn into the cell grid
//! over the application, through the same rasterizer a canvas uses.

use std::collections::HashMap;

use rustnative_core::inspect::{InspectBackend, Lifetimes, RealizedObject, node_name};
use rustnative_core::{NodeId, PlatformCapabilities, Rect, WindowId};
use rustnative_drawlist::GridMapping;

use crate::model::TuiModel;

/// The terminal backend's answers over its realized windows.
#[derive(Debug, Clone, Copy)]
pub struct TuiInspect<'a> {
    models: &'a HashMap<WindowId, TuiModel>,
    capabilities: &'a PlatformCapabilities,
    mapping: GridMapping,
}

impl<'a> TuiInspect<'a> {
    /// Answers over `models`, advertising `capabilities`.
    #[must_use]
    pub const fn new(
        models: &'a HashMap<WindowId, TuiModel>,
        capabilities: &'a PlatformCapabilities,
        mapping: GridMapping,
    ) -> Self {
        Self { models, capabilities, mapping }
    }

    /// Every realized node of `window` at its window rectangle (what the
    /// overlay is drawn over).
    #[must_use]
    pub fn window_rects(&self, window: WindowId) -> HashMap<NodeId, Rect> {
        self.models
            .get(&window)
            .map(|model| model.nodes().map(|node| (node.id, node.window_rect)).collect())
            .unwrap_or_default()
    }
}

impl InspectBackend for TuiInspect<'_> {
    fn name(&self) -> &'static str {
        "terminal"
    }

    fn realized(&self, window: WindowId) -> Vec<RealizedObject> {
        let Some(model) = self.models.get(&window) else { return Vec::new() };
        let mut objects: Vec<RealizedObject> = model
            .nodes()
            .map(|node| {
                let cells = node.cells;
                RealizedObject {
                    node: node_name(node.id),
                    key: node.key.clone(),
                    host_type: format!(
                        "cells {:?} [{},{} {}x{}] at {}x{} px per cell",
                        node.kind,
                        cells.x,
                        cells.y,
                        cells.width,
                        cells.height,
                        self.mapping.unit_width(),
                        self.mapping.unit_height()
                    ),
                    handle: None,
                    rect: Some([node.rect.x, node.rect.y, node.rect.width, node.rect.height]),
                }
            })
            .collect();
        objects.sort_by(|a, b| a.node.cmp(&b.node));
        objects
    }

    fn rects(&self, window: WindowId) -> Option<HashMap<NodeId, Rect>> {
        let model = self.models.get(&window)?;
        Some(model.nodes().map(|node| (node.id, node.rect)).collect())
    }

    fn lifetimes(&self) -> Lifetimes {
        let (created, destroyed) =
            self.models.values().fold((0, 0), |(created, destroyed), model| {
                (created + model.stats().created, destroyed + model.stats().destroyed)
            });
        Lifetimes {
            created,
            destroyed,
            live: created.saturating_sub(destroyed),
            recent: Vec::new(),
        }
    }

    fn capabilities(&self) -> PlatformCapabilities {
        self.capabilities.clone()
    }

    fn style_capabilities(&self) -> rustnative_style::StyleCapabilities {
        rustnative_style::TUI
    }

    fn unit_mapping(&self) -> Option<rustnative_style::UnitMapping> {
        Some(rustnative_style::TUI_UNITS)
    }
}
