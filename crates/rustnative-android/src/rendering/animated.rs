//! The renderer's half of animation: finding the transitions a render or a
//! layout started, and applying a frame's values to views.
//!
//! Geometry is animated in the coordinates views are *placed* at (after
//! right-to-left mirroring), since that is what a frame moves.

use std::collections::HashMap;

use rustnative_core::{
    AnimatedProperty, AnimatedValue, Frame, NodeId, Point, Rect, Size, Transition, TreeSnapshot,
};

use super::controls;
use super::realization::Renderer;
use crate::Error;
use crate::units::to_px;

/// A transition the renderer found and the window's timeline will start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransitionRequest {
    pub(crate) node: NodeId,
    pub(crate) property: AnimatedProperty,
    pub(crate) from: AnimatedValue,
    pub(crate) to: AnimatedValue,
    pub(crate) transition: Transition,
}

fn size_of(rect: Rect) -> Size {
    Size::new(
        u32::try_from(rect.width.max(0)).unwrap_or(0),
        u32::try_from(rect.height.max(0)).unwrap_or(0),
    )
}

impl Renderer {
    /// Transitions found since this was last called.
    pub(crate) fn take_transitions(&mut self) -> Vec<TransitionRequest> {
        std::mem::take(&mut self.pending_transitions)
    }

    /// Nodes removed since this was last called.
    pub(crate) fn forgotten_nodes(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.removed_nodes)
    }

    /// Finds the appearance transitions `next` starts against the tree on
    /// screen, and pins each at its starting value so applying `next` does
    /// not make it jump before the first frame.
    pub(crate) fn collect_appearance_transitions(&mut self, next: &TreeSnapshot) {
        let mut found = Vec::new();
        for node in next.nodes() {
            if node.transitions.is_empty() {
                continue;
            }
            let Some(previous) = self.snapshot.get(node.id) else { continue };
            let overrides = self.animated.get(node.id);
            for declared in &node.transitions {
                let (from, to) = match declared.property {
                    AnimatedProperty::Opacity => (
                        AnimatedValue::Scalar(overrides.opacity.unwrap_or(previous.opacity)),
                        AnimatedValue::Scalar(node.opacity),
                    ),
                    AnimatedProperty::Background => {
                        let before = overrides
                            .background
                            .or(previous.visual_style.properties().background_override());
                        match (before, node.visual_style.properties().background_override()) {
                            (Some(before), Some(after)) => {
                                (AnimatedValue::Color(before), AnimatedValue::Color(after))
                            }
                            _ => continue,
                        }
                    }
                    AnimatedProperty::Foreground => {
                        let before = overrides
                            .foreground
                            .or(previous.visual_style.properties().foreground_override());
                        match (before, node.visual_style.properties().foreground_override()) {
                            (Some(before), Some(after)) => {
                                (AnimatedValue::Color(before), AnimatedValue::Color(after))
                            }
                            _ => continue,
                        }
                    }
                    _ => continue,
                };
                if from != to {
                    found.push(TransitionRequest {
                        node: node.id,
                        property: declared.property,
                        from,
                        to,
                        transition: declared.transition,
                    });
                }
            }
        }
        for request in &found {
            self.animated.set(request.node, request.property, Some(request.from));
        }
        self.pending_transitions.extend(found);
    }

    /// Finds the geometry transitions a layout (placing views at `next`)
    /// starts, including matched geometry, and pins each at its start.
    pub(crate) fn collect_geometry_transitions(&mut self, next: &HashMap<NodeId, Rect>) {
        let mut geometry = Vec::new();
        for node in self.snapshot.nodes() {
            if node.transitions.is_empty() {
                continue;
            }
            let (Some(previous), Some(new)) =
                (self.placed.get(&node.id).copied(), next.get(&node.id).copied())
            else {
                continue;
            };
            let overrides = self.animated.get(node.id);
            for declared in &node.transitions {
                let (from, to) = match declared.property {
                    AnimatedProperty::Position => (
                        AnimatedValue::Offset(
                            overrides.position.unwrap_or(Point::new(previous.x, previous.y)),
                        ),
                        AnimatedValue::Offset(Point::new(new.x, new.y)),
                    ),
                    AnimatedProperty::Size => (
                        AnimatedValue::Size(overrides.size.unwrap_or(size_of(previous))),
                        AnimatedValue::Size(size_of(new)),
                    ),
                    _ => continue,
                };
                if from != to {
                    geometry.push(TransitionRequest {
                        node: node.id,
                        property: declared.property,
                        from,
                        to,
                        transition: declared.transition,
                    });
                }
            }
        }
        // Matched geometry: the arriving node moves from the leaving node's
        // rectangle.
        for matched in std::mem::take(&mut self.matched) {
            let Some(new) = next.get(&matched.node).copied() else { continue };
            let from = matched.from;
            geometry.push(TransitionRequest {
                node: matched.node,
                property: AnimatedProperty::Position,
                from: AnimatedValue::Offset(Point::new(from.x, from.y)),
                to: AnimatedValue::Offset(Point::new(new.x, new.y)),
                transition: matched.transition,
            });
            geometry.push(TransitionRequest {
                node: matched.node,
                property: AnimatedProperty::Size,
                from: AnimatedValue::Size(size_of(from)),
                to: AnimatedValue::Size(size_of(new)),
                transition: matched.transition,
            });
        }
        for request in &geometry {
            self.animated.set(request.node, request.property, Some(request.from));
        }
        self.pending_transitions.extend(geometry);
    }

    /// A property's value right now: what is animating, or what the tree
    /// and layout say — where an explicit animation starts from.
    pub(crate) fn current_value(
        &self,
        node: NodeId,
        property: AnimatedProperty,
    ) -> Option<AnimatedValue> {
        let overrides = self.animated.get(node);
        let rendered = self.snapshot.get(node)?;
        Some(match property {
            AnimatedProperty::Position => {
                let rect = self.placed.get(&node).copied()?;
                AnimatedValue::Offset(overrides.position.unwrap_or(Point::new(rect.x, rect.y)))
            }
            AnimatedProperty::Size => {
                let rect = self.placed.get(&node).copied()?;
                AnimatedValue::Size(overrides.size.unwrap_or(size_of(rect)))
            }
            AnimatedProperty::Translation => {
                AnimatedValue::Offset(overrides.translation.unwrap_or(Point::new(0, 0)))
            }
            AnimatedProperty::Opacity => {
                AnimatedValue::Scalar(overrides.opacity.unwrap_or(rendered.opacity))
            }
            AnimatedProperty::Background => AnimatedValue::Color(
                overrides
                    .background
                    .or(rendered.visual_style.properties().background_override())?,
            ),
            AnimatedProperty::Foreground => AnimatedValue::Color(
                overrides
                    .foreground
                    .or(rendered.visual_style.properties().foreground_override())?,
            ),
            _ => return None,
        })
    }

    /// Applies one frame to its view, if anything changed.
    pub(crate) fn apply_animation_frame(&mut self, frame: &Frame) -> Result<(), Error> {
        if !self.animated.set(frame.node, frame.property, frame.value) {
            return Ok(());
        }
        let _muted = super::realization::Muted::new();
        match frame.property {
            AnimatedProperty::Position | AnimatedProperty::Size | AnimatedProperty::Translation => {
                self.place_node(frame.node)
            }
            AnimatedProperty::Opacity => self.apply_opacity(frame.node),
            AnimatedProperty::Background | AnimatedProperty::Foreground => {
                if let Some(node) = self.snapshot.get(frame.node).cloned() {
                    if let Some(object) = self.registry.get_mut(node.id) {
                        // The animated colours are applied over the style:
                        // the style is sent again when the animation ends.
                        object.style = None;
                    }
                    self.apply_appearance(&node)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Places one node's view at its laid-out rectangle, with whatever is
    /// animating its geometry applied.
    pub(crate) fn place_node(&mut self, id: NodeId) -> Result<(), Error> {
        let Some(rect) = self.placed.get(&id).copied() else { return Ok(()) };
        let rect = self.animated.rect_for(id, rect);
        let Some(object) = self.registry.get(id) else { return Ok(()) };
        let density = self.density;
        let (left, top) = (to_px(rect.x, density), to_px(rect.y, density));
        let (right, bottom) = (
            to_px(rect.x.saturating_add(rect.width), density),
            to_px(rect.y.saturating_add(rect.height), density),
        );
        controls::place(&object.view, left, top, (right - left).max(0), (bottom - top).max(0))
    }

    /// Realizes a node's opacity: its own, or whatever animates it.
    pub(crate) fn apply_opacity(&self, id: NodeId) -> Result<(), Error> {
        let Some(object) = self.registry.get(id) else { return Ok(()) };
        let declared = self.snapshot.get(id).map_or(1.0, |node| node.opacity.get());
        let opacity = self.animated.get(id).opacity.map_or(declared, rustnative_core::Scalar::get);
        controls::set_alpha(&object.view, opacity)
    }
}
