//! Per-property mappers (`C24`) for the Web: the escape hatch at the
//! granularity applications need, as each native backend has it.
//!
//! Every property the realizer writes on an element — its text, its
//! classes, its accessibility attributes, whether it is hidden — goes
//! through a mapper for that `(node kind, property)`. The built-in mapping is
//! the default; an application may **extend** it (its function runs after,
//! with the element, to add what the portable model has no word for — an
//! attribute a browser extension or a test harness reads) or **replace** it
//! (its function runs instead). A mapper targets every node of a kind, or one
//! node by its key.
//!
//! Registrations are process-wide — pages are rendered on whichever worker
//! thread takes the request — and apply to everything the server realizes:
//! pages, and server-interactive sessions, which the server encodes too. A
//! client-subset island re-renders in the browser with the runtime's own
//! realizer, which these do not reach. [`active_mappers`] lists them, as the
//! inspector does for a native backend.
//!
//! ```
//! use rustnative_core::NodeKind;
//! use rustnative_web::mappers::{MappedProperty, MapperMode, MapperTarget, register_mapper};
//!
//! register_mapper(MapperTarget::Kind(NodeKind::Button), MappedProperty::Accessibility, MapperMode::Extend, |context| {
//!     context.element.set("data-test", "button");
//! });
//! # rustnative_web::mappers::clear_mappers();
//! ```

use std::sync::{Arc, PoisonError, RwLock};

use rustnative_core::{Node, NodeKind};

use crate::dom::{Child, Element};

/// A property the realizer writes on an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MappedProperty {
    /// A label's or button's caption: the element's text.
    Text,
    /// Its classes (layout, visual style, declarations).
    Style,
    /// Its role and `aria-*` attributes.
    Accessibility,
    /// Shown or hidden (`hidden`).
    Visibility,
}

/// What a mapper applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapperTarget {
    /// Every node of this kind.
    Kind(NodeKind),
    /// The one node created with this key.
    Key(String),
}

/// How a registered mapper relates to the built-in one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapperMode {
    /// Runs after the built-in mapping.
    Extend,
    /// Runs instead of the built-in mapping.
    Replace,
}

/// What a mapper is given.
#[derive(Debug)]
pub struct MapperContext<'a> {
    /// The element realizing the node, as it will be written.
    pub element: &'a mut Element,
    /// The node being realized.
    pub node: &'a Node,
}

/// A registered mapper, as [`active_mappers`] lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapperInfo {
    /// What it applies to.
    pub target: MapperTarget,
    /// The property.
    pub property: MappedProperty,
    /// Extend or replace.
    pub mode: MapperMode,
}

type MapperFn = Arc<dyn Fn(&mut MapperContext<'_>) + Send + Sync>;

struct Registration {
    info: MapperInfo,
    mapper: MapperFn,
}

static MAPPERS: RwLock<Vec<Registration>> = RwLock::new(Vec::new());

/// Registers `mapper` for `property` of `target`, replacing an earlier one
/// for the same target, property, and mode.
pub fn register_mapper(
    target: MapperTarget,
    property: MappedProperty,
    mode: MapperMode,
    mapper: impl Fn(&mut MapperContext<'_>) + Send + Sync + 'static,
) {
    let info = MapperInfo { target, property, mode };
    let mut mappers = MAPPERS.write().unwrap_or_else(PoisonError::into_inner);
    mappers.retain(|existing| existing.info != info);
    mappers.push(Registration { info, mapper: Arc::new(mapper) });
}

/// Removes every registered mapper.
pub fn clear_mappers() {
    MAPPERS.write().unwrap_or_else(PoisonError::into_inner).clear();
}

/// Every registered mapper.
#[must_use]
pub fn active_mappers() -> Vec<MapperInfo> {
    MAPPERS.read().unwrap_or_else(PoisonError::into_inner).iter().map(|r| r.info.clone()).collect()
}

/// The mappers for `property` of `node` in `mode`: for `Replace`, the most
/// specific one (a key-targeted one over a kind-targeted one); for `Extend`,
/// every one, kind first.
fn matching(node: &Node, property: MappedProperty, mode: MapperMode) -> Vec<MapperFn> {
    let key = node.id().local_key();
    let kind = node.kind();
    let mappers = MAPPERS.read().unwrap_or_else(PoisonError::into_inner);
    let (mut by_kind, mut by_key) = (Vec::new(), Vec::new());
    for registration in
        mappers.iter().filter(|r| r.info.property == property && r.info.mode == mode)
    {
        match &registration.info.target {
            MapperTarget::Kind(target) if *target == kind => {
                by_kind.push(Arc::clone(&registration.mapper));
            }
            MapperTarget::Key(target) if key.as_deref() == Some(target.as_str()) => {
                by_key.push(Arc::clone(&registration.mapper));
            }
            _ => {}
        }
    }
    match mode {
        MapperMode::Replace => by_key.into_iter().chain(by_kind).take(1).collect(),
        MapperMode::Extend => by_kind.into_iter().chain(by_key).collect(),
    }
}

/// Undoes the built-in mapping of `property` on `element`, for a mapper
/// that replaces it.
fn clear_property(element: &mut Element, property: MappedProperty) {
    match property {
        MappedProperty::Text => element.children.retain(|child| !matches!(child, Child::Text(_))),
        MappedProperty::Style => element.attrs.retain(|(name, _)| name != "class"),
        MappedProperty::Accessibility => {
            element.attrs.retain(|(name, _)| name != "role" && !name.starts_with("aria-"));
        }
        MappedProperty::Visibility => element.attrs.retain(|(name, _)| name != "hidden"),
    }
}

/// Runs the registered mappers on `element`, which the realizer has just
/// written for `node`. A no-op when none is registered.
pub(crate) fn apply(element: &mut Element, node: &Node) {
    if MAPPERS.read().unwrap_or_else(PoisonError::into_inner).is_empty() {
        return;
    }
    for property in [
        MappedProperty::Text,
        MappedProperty::Style,
        MappedProperty::Accessibility,
        MappedProperty::Visibility,
    ] {
        if let Some(replace) = matching(node, property, MapperMode::Replace).first() {
            clear_property(element, property);
            replace(&mut MapperContext { element: &mut *element, node });
        }
        for extend in matching(node, property, MapperMode::Extend) {
            extend(&mut MapperContext { element: &mut *element, node });
        }
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::LayoutStyle;

    use super::*;

    /// Registrations are process-wide and the library's tests run in
    /// parallel, so this test targets a key and a kind no other test
    /// realizes.
    #[test]
    fn mappers_extend_and_replace_what_the_realizer_writes() {
        register_mapper(
            MapperTarget::Key("mapped-go".into()),
            MappedProperty::Text,
            MapperMode::Replace,
            |context| context.element.children.push(Child::Text("[Go]".into())),
        );
        register_mapper(
            MapperTarget::Kind(NodeKind::Surface),
            MappedProperty::Accessibility,
            MapperMode::Extend,
            |context| context.element.set("data-mapped", "surface"),
        );
        let realize = |node: &Node| {
            let mut sheet = crate::css::StyleSheet::new();
            crate::dom::Realizer::new(&mut sheet, "", node).root(node)
        };
        let go = realize(&Node::button("mapped-go", "Go"));
        assert_eq!(go.children, vec![Child::Text("[Go]".into())], "replaced for its key");
        let other = realize(&Node::button("mapped-stop", "Stop"));
        assert_eq!(other.children, vec![Child::Text("Stop".into())], "other nodes keep theirs");
        for key in ["mapped-a", "mapped-b"] {
            let surface = realize(&Node::native_surface(key, LayoutStyle::new()));
            assert_eq!(surface.get("data-mapped"), Some("surface"), "extended for its kind");
        }
        assert!(active_mappers().len() >= 2);
    }
}
