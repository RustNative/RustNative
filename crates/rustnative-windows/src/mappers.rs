//! Per-property native mappers (`C24`): the escape hatch at the
//! granularity applications actually need.
//!
//! Every property this backend applies to a native control — its text,
//! its appearance, its accessibility annotations, its visibility — goes
//! through a mapper for that `(control kind, property)`. The built-in
//! mapper is the default; an application may **extend** it (its function
//! runs after the default, with the control's `HWND`, to add what the
//! portable model has no word for) or **replace** it (its function runs
//! instead, for a control the application wants to present its own way).
//! A mapper may target every control of a kind, or one node by the key its
//! author wrote.
//!
//! Registrations are per UI thread, made before or during `run`, and are
//! listed by [`active_mappers`] — what the inspector reports (`C24-2`), so
//! a customization is never invisible.
//!
//! ```no_run
//! use rustnative_core::NodeKind;
//! use rustnative_windows::{MappedProperty, MapperMode, MapperTarget, register_mapper};
//!
//! // Every button also gets a tooltip-worthy extended style — here, just a
//! // marker in the window's user data would do; any Win32 call is allowed
//! // on the `HWND`.
//! register_mapper(
//!     MapperTarget::Kind(NodeKind::Button),
//!     MappedProperty::Text,
//!     MapperMode::Extend,
//!     |context| {
//!         let _hwnd = context.hwnd;
//!     },
//! );
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::{NodeKind, TreeNode};

/// A property the backend applies to native controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MappedProperty {
    /// A label's or button's caption, a text field's value.
    Text,
    /// Font and colours.
    Style,
    /// Accessible name, role, and description annotations.
    Accessibility,
    /// Shown or hidden.
    Visibility,
}

/// What a mapper applies to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapperTarget {
    /// Every control of this kind.
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
    /// The control's `HWND`, as an integer.
    pub hwnd: isize,
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

/// Applies `property` to `hwnd` for `node`: the replacement if one is
/// registered, otherwise `built_in`; then every extension. Returns what the
/// built-in mapper returned, or `replaced` when it was replaced.
#[cfg(windows)]
pub(crate) fn apply<R>(
    hwnd: windows_sys::Win32::Foundation::HWND,
    node: &TreeNode,
    property: MappedProperty,
    replaced: R,
    built_in: impl FnOnce() -> R,
) -> R {
    let context = MapperContext { hwnd: hwnd as isize, node };
    let replacement = matching(node, property, MapperMode::Replace);
    let result = if let Some(replace) = replacement.first() {
        replace(&context);
        replaced
    } else {
        built_in()
    };
    for extend in matching(node, property, MapperMode::Extend) {
        extend(&context);
    }
    result
}
