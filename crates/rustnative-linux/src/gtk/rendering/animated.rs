//! The renderer's half of animation: finding the transitions a render or a
//! layout started, and applying a frame's values to widgets.
//!
//! Geometry is animated in the coordinates widgets are *placed* at (after
//! right-to-left mirroring), since that is what a frame moves.

use std::collections::HashMap;
use std::fmt::Write as _;

use gtk::prelude::*;
use rustnative_core::{
    AnimatedProperty, AnimatedValue, Frame, NodeId, Point, Rect, Size, Transition, TreeSnapshot,
};

use super::realization::Renderer;
use super::styling;
use crate::gtk::layout_widget::RnLayout;

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
                        let after = node.visual_style.properties().background_override();
                        match (before, after) {
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
                        let after = node.visual_style.properties().foreground_override();
                        match (before, after) {
                            (Some(before), Some(after)) => {
                                (AnimatedValue::Color(before), AnimatedValue::Color(after))
                            }
                            _ => continue,
                        }
                    }
                    // Geometry is found by layout.
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

    /// Finds the geometry transitions a layout (placing widgets at `next`)
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
        // Matched geometry: GTK has no shared-element transition of its
        // own, so the arriving node moves from the leaving node's rectangle.
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
            // A property this backend does not realize: nothing to start from.
            _ => return None,
        })
    }

    /// Applies one frame to its widget, if anything changed.
    pub(crate) fn apply_animation_frame(&mut self, frame: &Frame, window_root: &RnLayout) {
        if !self.animated.set(frame.node, frame.property, frame.value) {
            return;
        }
        match frame.property {
            AnimatedProperty::Position | AnimatedProperty::Size | AnimatedProperty::Translation => {
                self.place_node(frame.node, window_root);
            }
            AnimatedProperty::Opacity => self.apply_opacity(frame.node),
            AnimatedProperty::Background | AnimatedProperty::Foreground => {
                if let Some(node) = self.snapshot.get(frame.node).cloned() {
                    self.apply_style_class(&node);
                }
            }
            _ => {}
        }
    }

    /// Places one node's widget at its laid-out rectangle, with whatever is
    /// animating its geometry applied.
    pub(crate) fn place_node(&mut self, id: NodeId, window_root: &RnLayout) {
        let Some(rect) = self.placed.get(&id).copied() else { return };
        let Some(parent) = self.snapshot.get(id).map(|node| node.parent) else { return };
        let rect = self.animated.rect_for(id, rect);
        let container = self.content_of(parent, window_root);
        if let Some(object) = self.registry.get(id) {
            container.place(&object.widget, rect);
        }
    }

    /// Realizes a node's opacity: its own, or whatever animates it.
    pub(crate) fn apply_opacity(&self, id: NodeId) {
        let Some(object) = self.registry.get(id) else { return };
        let declared = self.snapshot.get(id).map_or(1.0, |node| node.opacity.get());
        let opacity =
            f64::from(self.animated.get(id).opacity.map_or(declared, rustnative_core::Scalar::get));
        // GTK keeps opacity as an 8-bit alpha: closer than a step is equal.
        if (object.widget.opacity() - opacity).abs() > 0.5 / 255.0 {
            object.widget.set_opacity(opacity);
        }
    }

    /// Realizes a node's style class and, while its colours animate, the
    /// frame's colours.
    ///
    /// Every frame of a colour transition is a colour no rule has yet, so
    /// frames do not go through the shared style sheet (which would grow by
    /// a rule a frame and re-parse on each): the animated colours are one
    /// small provider on the widget alone, replaced each frame and removed
    /// when the animation ends.
    pub(crate) fn apply_style_class(&mut self, node: &rustnative_core::TreeNode) {
        let Some(object) = self.registry.get(node.id) else { return };
        let class =
            self.styles.borrow_mut().class_for(node.kind, &node.style_override, &self.theme);
        styling::apply_class(&object.widget, class.as_deref());
        self.styles.borrow_mut().flush();
        let overrides = self.animated.get(node.id);
        let mut css = String::new();
        if let Some(foreground) = overrides.foreground {
            let _ = write!(css, "color: {}; ", styling::css_color(foreground));
        }
        if let Some(background) = overrides.background {
            let _ = write!(
                css,
                "background-color: {}; background-image: none; ",
                styling::css_color(background)
            );
        }
        let widget = object.widget.clone();
        if css.is_empty() {
            if let Some(provider) = self.animation_providers.remove(&node.id) {
                remove_widget_provider(&widget, &provider);
            }
            return;
        }
        let provider = self.animation_providers.entry(node.id).or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            add_widget_provider(&widget, &provider);
            provider
        });
        styling::load(provider, &format!("* {{ {css}}}"));
    }
}

#[allow(
    deprecated,
    reason = "a style provider for one widget alone: GTK 4.14 has no replacement for per-widget providers"
)]
fn add_widget_provider(widget: &gtk::Widget, provider: &gtk::CssProvider) {
    widget.style_context().add_provider(provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
}

#[allow(deprecated, reason = "the counterpart of `add_widget_provider`")]
fn remove_widget_provider(widget: &gtk::Widget, provider: &gtk::CssProvider) {
    widget.style_context().remove_provider(provider);
}
