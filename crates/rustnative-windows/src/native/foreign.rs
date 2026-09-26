//! Foreign native objects adopted as leaves of the tree (embedding outward,
//! `PLAN.md` Milestone 40): a control the framework did not write, created
//! by a factory the application registered, then measured, laid out,
//! clipped, and destroyed by the framework's own rules.
//!
//! The factory runs on the UI thread when a [`rustnative_core::Node::foreign`]
//! node of its kind is inserted, and creates the object as a child of the
//! window it is given. The object's accessibility is its own: a system
//! control already answers UI Automation, and the framework neither
//! subclasses it nor changes its tab stop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rustnative_core::{NodeId, Size};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, HWND_MESSAGE, IsWindow, SW_HIDE, SetParent, ShowWindow,
};

use super::win32::{best_effort, ignored_by_contract};
use crate::Error;
use crate::error::NativeContext;

/// Who owns a foreign object once the framework has adopted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// The framework: removing the node destroys the object.
    Owned,
    /// The application: removing the node hides the object and parks it
    /// under a message-only parent, and the application destroys it.
    Borrowed,
}

/// What a foreign factory hands the framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignControl {
    /// The object's window, created as a child of the parent the factory
    /// was given.
    pub hwnd: HWND,
    /// Who destroys it.
    pub ownership: Ownership,
}

type Create = Rc<dyn Fn(HWND) -> Option<ForeignControl>>;

struct Factory {
    preferred: Size,
    create: Create,
}

thread_local! {
    static FACTORIES: RefCell<HashMap<String, Factory>> = RefCell::new(HashMap::new());
}

/// Registers the factory for foreign nodes of `kind` on this (the UI)
/// thread. `preferred` is the object's natural size, which layout uses
/// where the node does not fix one; `create` makes the object as a child of
/// the window it is given and returns it, or `None` if it could not.
pub fn register_foreign(
    kind: impl Into<String>,
    preferred: Size,
    create: impl Fn(HWND) -> Option<ForeignControl> + 'static,
) {
    FACTORIES.with(|factories| {
        factories.borrow_mut().insert(kind.into(), Factory { preferred, create: Rc::new(create) });
    });
}

/// The natural size of a registered kind.
pub(crate) fn preferred_size(kind: &str) -> Option<Size> {
    if let Some(content) = rustnative_core::HostContent::from_kind(kind) {
        return Some(super::host_content::preferred_size(&content));
    }
    FACTORIES.with(|factories| factories.borrow().get(kind).map(|factory| factory.preferred))
}

/// Creates the object for a node of `kind` inside `parent`.
pub(crate) fn create(kind: &str, parent: HWND, node: NodeId) -> Result<ForeignControl, Error> {
    // Cloned out so the factory runs without the registry borrowed: it may
    // create windows, whose messages may reach code that registers more.
    let create = FACTORIES
        .with(|factories| factories.borrow().get(kind).map(|factory| Rc::clone(&factory.create)));
    let context = NativeContext::none().with_node(node);
    // Host content (`C28`) is the backend's own; an application factory
    // registered for the exact kind still wins.
    let create = create.or_else(|| {
        let host = rustnative_core::HostContent::from_kind(kind)?;
        let create: Create = Rc::new(move |parent| super::host_content::create(&host, parent));
        Some(create)
    });
    let Some(create) = create else {
        return Err(Error::ForeignUnavailable {
            kind: kind.to_owned(),
            reason: "no factory is registered",
            context,
        });
    };
    let control = create(parent).ok_or_else(|| Error::ForeignUnavailable {
        kind: kind.to_owned(),
        reason: "the factory created nothing",
        context,
    })?;
    // SAFETY: `IsWindow` accepts any handle value.
    if control.hwnd.is_null() || unsafe { IsWindow(control.hwnd) } == 0 {
        return Err(Error::ForeignUnavailable {
            kind: kind.to_owned(),
            reason: "the factory returned no window",
            context,
        });
    }
    Ok(control)
}

/// Gives up a foreign object: destroys an owned one; hides a borrowed one
/// and parks it under a message-only parent, so destroying the framework's
/// windows does not destroy it.
pub(crate) fn release(hwnd: HWND, ownership: Ownership) {
    // SAFETY: `IsWindow` accepts any handle value.
    if unsafe { IsWindow(hwnd) } == 0 {
        return;
    }
    match ownership {
        Ownership::Owned => {
            // SAFETY: an owned foreign object is the framework's to destroy,
            // exactly once, here.
            best_effort(
                unsafe { DestroyWindow(hwnd) } != 0,
                "DestroyWindow(foreign)",
                "the object leaks",
            );
        }
        Ownership::Borrowed => {
            // SAFETY: `hwnd` is live; hiding and reparenting take no
            // pointers beyond the handles.
            unsafe {
                ignored_by_contract(ShowWindow(hwnd, SW_HIDE));
                best_effort(
                    !SetParent(hwnd, HWND_MESSAGE).is_null(),
                    "SetParent(foreign, HWND_MESSAGE)",
                    "the borrowed object is destroyed with its former parent",
                );
            }
        }
    }
}
