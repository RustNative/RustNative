//! The headless backend's answers to the inspection protocol (`PLAN.md`
//! Milestone 44): its realized model is the "host", so every question the
//! protocol asks of a backend has an exact answer here.

use std::collections::HashMap;

use rustnative_core::inspect::{InspectBackend, Lifetimes, RealizedObject, node_name};
use rustnative_core::{NodeId, Platform, PlatformCapabilities, Rect, WindowId};

use crate::platform::HeadlessPlatform;
use crate::tree::HeadlessTree;

/// The headless backend, answering over its realized windows.
#[derive(Debug, Clone, Copy)]
pub struct HeadlessInspect<'a> {
    trees: &'a HashMap<WindowId, HeadlessTree>,
}

impl<'a> HeadlessInspect<'a> {
    /// Answers over `trees`.
    #[must_use]
    pub const fn new(trees: &'a HashMap<WindowId, HeadlessTree>) -> Self {
        Self { trees }
    }

    /// Every realized node of `window` at its window rectangle — what the
    /// overlay is drawn over.
    #[must_use]
    pub fn window_rects(&self, window: WindowId) -> HashMap<NodeId, Rect> {
        self.trees
            .get(&window)
            .map(|tree| tree.nodes().map(|node| (node.id, node.window_rect)).collect())
            .unwrap_or_default()
    }
}

impl InspectBackend for HeadlessInspect<'_> {
    fn name(&self) -> &'static str {
        "headless"
    }

    fn realized(&self, window: WindowId) -> Vec<RealizedObject> {
        let Some(tree) = self.trees.get(&window) else { return Vec::new() };
        let mut objects: Vec<RealizedObject> = tree
            .nodes()
            .map(|node| RealizedObject {
                node: node_name(node.id),
                key: node.key.clone(),
                host_type: format!("headless {:?}", node.kind),
                handle: None,
                rect: Some([node.rect.x, node.rect.y, node.rect.width, node.rect.height]),
            })
            .collect();
        objects.sort_by(|a, b| a.node.cmp(&b.node));
        objects
    }

    fn rects(&self, window: WindowId) -> Option<HashMap<NodeId, Rect>> {
        let tree = self.trees.get(&window)?;
        Some(tree.nodes().map(|node| (node.id, node.rect)).collect())
    }

    fn lifetimes(&self) -> Lifetimes {
        let (created, destroyed) =
            self.trees.values().fold((0, 0), |(created, destroyed), tree| {
                (created + tree.stats().created, destroyed + tree.stats().destroyed)
            });
        Lifetimes {
            created,
            destroyed,
            live: created.saturating_sub(destroyed),
            recent: Vec::new(),
        }
    }

    fn capabilities(&self) -> PlatformCapabilities {
        HeadlessPlatform::new().capabilities()
    }

    fn style_capabilities(&self) -> rustnative_style::StyleCapabilities {
        rustnative_style::HEADLESS
    }

    fn unit_mapping(&self) -> Option<rustnative_style::UnitMapping> {
        Some(rustnative_style::HEADLESS_UNITS)
    }
}
