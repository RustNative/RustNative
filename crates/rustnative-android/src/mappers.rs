//! Per-property native mappers (`C24`): the escape hatch at the
//! granularity applications need — the same contract as the Windows and
//! Linux backends', on Android views.
//!
//! Every property this backend applies to a view — its text, its style, its
//! accessibility annotations, its visibility — goes through a mapper for
//! that `(node kind, property)`. The built-in mapper is the default; an
//! application may **extend** it (its function runs after the default) or
//! **replace** it. A mapper may target every node of a kind, or one node by
//! its key.
//!
//! A mapper receives the view as a [`NativeView`]: a JNI global reference
//! the application calls with its own JNI bindings (`NativeView::raw` and
//! [`crate::java_vm`]), on the main thread, during the call.

use std::cell::RefCell;
use std::rc::Rc;

use rustnative_core::{NodeKind, TreeNode};

use crate::Error;
use crate::jni_host::JavaRef;

/// A property the backend applies to views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MappedProperty {
    /// A label's or button's caption, a text field's value.
    Text,
    /// Font, colours, borders.
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

/// An Android view, as a mapper or a foreign factory sees it.
#[derive(Debug, Clone)]
pub struct NativeView(pub(crate) JavaRef);

impl NativeView {
    /// The view (or other Java object) a JNI reference refers to, held by
    /// the framework from now on — how a foreign factory hands over a view
    /// it made with its own JNI calls.
    ///
    /// # Safety
    ///
    /// `raw` is a live `jobject` (local or global) valid on the calling
    /// thread, which is the main thread.
    #[must_use]
    pub unsafe fn from_raw(raw: *mut std::ffi::c_void) -> Option<Self> {
        // SAFETY: forwarded from this function's contract.
        unsafe { crate::jni_host::adopt_raw(raw) }.map(Self)
    }

    /// The view as a JNI `jobject` — a global reference, valid while this
    /// value lives, on any thread attached to the VM.
    #[must_use]
    pub fn raw(&self) -> *mut std::ffi::c_void {
        self.0.as_obj().as_raw().cast()
    }
}

/// What a mapper is given.
#[derive(Debug)]
pub struct MapperContext<'a> {
    /// The view realizing the node.
    pub view: &'a NativeView,
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
/// the same target, property, and mode replaces an earlier one. Mappers
/// are per main thread: register them from `main` before `run`.
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

/// Removes every registered mapper.
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

/// Every registered mapper.
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

/// Applies `property` to `view` for `node`: the replacement if one is
/// registered, otherwise `built_in`; then every extension.
pub(crate) fn apply(
    view: &JavaRef,
    node: &TreeNode,
    property: MappedProperty,
    built_in: impl FnOnce() -> Result<(), Error>,
) -> Result<(), Error> {
    let replacements = matching(node, property, MapperMode::Replace);
    let extensions = matching(node, property, MapperMode::Extend);
    if replacements.is_empty() && extensions.is_empty() {
        return built_in();
    }
    let native = NativeView(view.clone());
    let context = MapperContext { view: &native, node };
    if let Some(replace) = replacements.first() {
        replace(&context);
    } else {
        built_in()?;
    }
    for extend in extensions {
        extend(&context);
    }
    Ok(())
}
