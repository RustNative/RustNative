//! Per-property native mappers (`C24`): the escape hatch at the
//! granularity applications actually need — the same contract as the
//! Windows backend's, on GTK widgets.
//!
//! Every property this backend applies to a widget — its text, its style,
//! its accessibility annotations, its visibility — goes through a mapper
//! for that `(node kind, property)`. The built-in mapper is the default; an
//! application may **extend** it (its function runs after the default, with
//! the widget, to add what the portable model has no word for) or
//! **replace** it (its function runs instead). A mapper may target every
//! node of a kind, or one node by the key its author wrote.
//!
//! Registrations are per GTK thread, made before or during `run`, and are
//! listed by [`active_mappers`] — what the inspector reports (`C24-2`).
//!
//! ```no_run
//! use gtk::prelude::*;
//! use rustnative_core::NodeKind;
//! use rustnative_linux::{MappedProperty, MapperMode, MapperTarget, register_mapper};
//!
//! // Every button also gets a tooltip, which the portable model does not
//! // declare.
//! register_mapper(
//!     MapperTarget::Kind(NodeKind::Button),
//!     MappedProperty::Text,
//!     MapperMode::Extend,
//!     |context| context.widget.set_tooltip_text(Some("Press me")),
//! );
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::{NodeKind, TreeNode};

/// A property the backend applies to widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MappedProperty {
    /// A label's or button's caption, a text field's value.
    Text,
    /// Font and colours.
    Style,
    /// Accessible name, role, description, and state annotations.
    Accessibility,
    /// Shown or hidden.
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
    /// Runs after the built-in mapper.
    Extend,
    /// Runs instead of the built-in mapper.
    Replace,
}

/// What a mapper is given.
#[derive(Debug)]
pub struct MapperContext<'a> {
    /// The widget realizing the node.
    pub widget: &'a gtk::Widget,
    /// The node being applied, as the backend sees it.
    pub node: &'a TreeNode,
}

type MapperFn = Rc<dyn Fn(&MapperContext<'_>)>;

struct Registration {
    target: MapperTarget,
    property: MappedProperty,
    mode: MapperMode,
    mapper: MapperFn,
}

thread_local! {
    static MAPPERS: RefCell<Vec<Registration>> = const { RefCell::new(Vec::new()) };
}

/// Registers `mapper` for `property` on `target`. A later registration for
/// the same target, property, and mode replaces an earlier one.
pub fn register_mapper(
    target: MapperTarget,
    property: MappedProperty,
    mode: MapperMode,
    mapper: impl Fn(&MapperContext<'_>) + 'static,
) {
    MAPPERS.with(|mappers| {
        let mut mappers = mappers.borrow_mut();
        mappers.retain(|existing| {
            !(existing.target == target && existing.property == property && existing.mode == mode)
        });
        mappers.push(Registration { target, property, mode, mapper: Rc::new(mapper) });
    });
}

/// Removes every registered mapper on this thread.
pub fn clear_mappers() {
    MAPPERS.with(|mappers| mappers.borrow_mut().clear());
}

/// One active customization, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapperInfo {
    /// What it applies to.
    pub target: MapperTarget,
    /// Which property.
    pub property: MappedProperty,
    /// Extend or replace.
    pub mode: MapperMode,
}

/// Every mapper registered on this thread.
#[must_use]
pub fn active_mappers() -> Vec<MapperInfo> {
    MAPPERS.with(|mappers| {
        mappers
            .borrow()
            .iter()
            .map(|registration| MapperInfo {
                target: registration.target.clone(),
                property: registration.property,
                mode: registration.mode,
            })
            .collect()
    })
}

fn matching(node: &TreeNode, property: MappedProperty, mode: MapperMode) -> Vec<MapperFn> {
    let key = node.id.local_key();
    MAPPERS.with(|mappers| {
        let mappers = mappers.borrow();
        // A key-targeted mapper is more specific than a kind-targeted one,
        // so for `Replace` it wins; both run for `Extend`, kind first.
        let mut kind: Vec<MapperFn> = Vec::new();
        let mut keyed: Vec<MapperFn> = Vec::new();
        for registration in mappers.iter().filter(|r| r.property == property && r.mode == mode) {
            match &registration.target {
                MapperTarget::Kind(target) if *target == node.kind => {
                    kind.push(Rc::clone(&registration.mapper));
                }
                MapperTarget::Key(target) if key.as_deref() == Some(target.as_str()) => {
                    keyed.push(Rc::clone(&registration.mapper));
                }
                _ => {}
            }
        }
        match mode {
            MapperMode::Replace => keyed.into_iter().chain(kind).take(1).collect(),
            MapperMode::Extend => kind.into_iter().chain(keyed).collect(),
        }
    })
}

/// Applies `property` to `widget` for `node`: the replacement if one is
/// registered, otherwise `built_in`; then every extension.
pub(crate) fn apply(
    widget: &gtk::Widget,
    node: &TreeNode,
    property: MappedProperty,
    built_in: impl FnOnce(),
) {
    let context = MapperContext { widget, node };
    if let Some(replace) = matching(node, property, MapperMode::Replace).first() {
        replace(&context);
    } else {
        built_in();
    }
    for extend in matching(node, property, MapperMode::Extend) {
        extend(&context);
    }
}
