//! Where an animation's per-frame values live between the timeline and the
//! native objects.
//!
//! A frame does not change the declarative tree — that is the whole point
//! (see `rustnative_core::animation`) — so the values it produces have to be
//! held *beside* the tree and consulted wherever the backend would
//! otherwise use the rendered value. That is this type: geometry consults
//! it in `Renderer::position_node`, appearance in
//! `Renderer::apply_control_style`.
//!
//! Every override is optional and resets to `None` when its animation ends,
//! at which point the node goes back to exactly what the rendered tree
//! says. Nothing here outlives its node.

use std::collections::HashMap;

use rustnative_core::{AnimatedProperty, AnimatedValue, Color, NodeId, Point, Rect, Scalar, Size};

/// One node's animated values, each `None` when nothing animates it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeOverrides {
    /// Replaces the laid-out position.
    pub(crate) position: Option<Point>,
    /// Replaces the laid-out size.
    pub(crate) size: Option<Size>,
    /// Added to the position (laid-out or overridden).
    pub(crate) translation: Option<Point>,
    /// Replaces the node's declared opacity.
    pub(crate) opacity: Option<Scalar>,
    /// Replaces the resolved background color.
    pub(crate) background: Option<Color>,
    /// Replaces the resolved foreground color.
    pub(crate) foreground: Option<Color>,
}

impl NodeOverrides {
    fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// Every animated node's current values.
#[derive(Debug, Default)]
pub(crate) struct AnimatedOverrides {
    nodes: HashMap<NodeId, NodeOverrides>,
}

impl AnimatedOverrides {
    /// This node's overrides (all unset if it is not animating).
    pub(crate) fn get(&self, node: NodeId) -> NodeOverrides {
        self.nodes.get(&node).copied().unwrap_or_default()
    }

    /// Records one frame's value — or clears the property, when the
    /// animation ended and the node returns to what the tree says.
    ///
    /// Returns whether anything actually changed, so a caller can skip a
    /// native call that would repaint the same pixels.
    pub(crate) fn set(
        &mut self,
        node: NodeId,
        property: AnimatedProperty,
        value: Option<AnimatedValue>,
    ) -> bool {
        let mut overrides = self.get(node);
        let before = overrides;
        match (property, value) {
            (AnimatedProperty::Position, Some(AnimatedValue::Offset(point))) => {
                overrides.position = Some(point);
            }
            (AnimatedProperty::Position, None) => overrides.position = None,
            (AnimatedProperty::Size, Some(AnimatedValue::Size(size))) => {
                overrides.size = Some(size);
            }
            (AnimatedProperty::Size, None) => overrides.size = None,
            (AnimatedProperty::Translation, Some(AnimatedValue::Offset(point))) => {
                overrides.translation = Some(point);
            }
            (AnimatedProperty::Translation, None) => overrides.translation = None,
            (AnimatedProperty::Opacity, Some(AnimatedValue::Scalar(alpha))) => {
                overrides.opacity = Some(alpha);
            }
            (AnimatedProperty::Opacity, None) => overrides.opacity = None,
            (AnimatedProperty::Background, Some(AnimatedValue::Color(color))) => {
                overrides.background = Some(color);
            }
            (AnimatedProperty::Background, None) => overrides.background = None,
            (AnimatedProperty::Foreground, Some(AnimatedValue::Color(color))) => {
                overrides.foreground = Some(color);
            }
            (AnimatedProperty::Foreground, None) => overrides.foreground = None,
            // A value of the wrong kind for its property cannot come from
            // the timeline, which only ever interpolates matching kinds;
            // a property this backend does not realize has nothing to hold
            // or to clear.
            (_, Some(_) | None) => return false,
        }
        if overrides == before {
            return false;
        }
        if overrides.is_empty() {
            self.nodes.remove(&node);
        } else {
            self.nodes.insert(node, overrides);
        }
        true
    }

    /// `layout` adjusted by this node's animated geometry.
    pub(crate) fn rect_for(&self, node: NodeId, layout: Rect) -> Rect {
        let overrides = self.get(node);
        let position = overrides.position.unwrap_or(Point::new(layout.x, layout.y));
        let translation = overrides.translation.unwrap_or(Point::new(0, 0));
        let (width, height) = match overrides.size {
            Some(size) => (dimension(size.width), dimension(size.height)),
            None => (layout.width, layout.height),
        };
        Rect::new(
            position.x.saturating_add(translation.x),
            position.y.saturating_add(translation.y),
            width,
            height,
        )
    }

    /// Drops one node's overrides.
    pub(crate) fn forget(&mut self, node: NodeId) {
        self.nodes.remove(&node);
    }
}

/// Converts an animated (unsigned) dimension to the signed one Win32
/// positioning uses, saturating rather than wrapping.
#[allow(clippy::cast_possible_wrap, reason = "clamped into i32's range first")]
fn dimension(value: u32) -> i32 {
    value.min(i32::MAX.unsigned_abs()) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> NodeId {
        NodeId::from_key("animated")
    }

    #[test]
    fn a_node_with_nothing_animating_keeps_its_laid_out_rectangle() {
        let overrides = AnimatedOverrides::default();
        let layout = Rect::new(10, 20, 30, 40);
        assert_eq!(overrides.rect_for(node(), layout), layout);
        assert_eq!(overrides.get(node()), NodeOverrides::default());
    }

    #[test]
    fn position_size_and_translation_compose() {
        let mut overrides = AnimatedOverrides::default();
        assert!(overrides.set(
            node(),
            AnimatedProperty::Position,
            Some(AnimatedValue::Offset(Point::new(5, 5)))
        ));
        assert!(overrides.set(
            node(),
            AnimatedProperty::Translation,
            Some(AnimatedValue::Offset(Point::new(2, -3)))
        ));
        assert!(overrides.set(
            node(),
            AnimatedProperty::Size,
            Some(AnimatedValue::Size(Size::new(8, 9)))
        ));
        assert_eq!(overrides.rect_for(node(), Rect::new(0, 0, 100, 100)), Rect::new(7, 2, 8, 9));
    }

    #[test]
    fn clearing_every_property_returns_the_node_to_the_rendered_tree() {
        let mut overrides = AnimatedOverrides::default();
        overrides.set(
            node(),
            AnimatedProperty::Translation,
            Some(AnimatedValue::Offset(Point::new(9, 9))),
        );
        assert_eq!(overrides.get(node()).translation, Some(Point::new(9, 9)));
        assert!(overrides.set(node(), AnimatedProperty::Translation, None));
        assert_eq!(overrides.get(node()), NodeOverrides::default());
        assert_eq!(overrides.rect_for(node(), Rect::new(1, 2, 3, 4)), Rect::new(1, 2, 3, 4));
    }

    #[test]
    fn setting_the_same_value_twice_reports_no_change() {
        let mut overrides = AnimatedOverrides::default();
        let value = Some(AnimatedValue::opacity(0.5));
        assert!(overrides.set(node(), AnimatedProperty::Opacity, value));
        assert!(!overrides.set(node(), AnimatedProperty::Opacity, value), "nothing to repaint");
    }

    #[test]
    fn a_removed_node_is_forgotten() {
        let mut overrides = AnimatedOverrides::default();
        overrides.set(node(), AnimatedProperty::Opacity, Some(AnimatedValue::opacity(0.2)));
        overrides.forget(node());
        assert_eq!(overrides.get(node()), NodeOverrides::default());
    }
}
