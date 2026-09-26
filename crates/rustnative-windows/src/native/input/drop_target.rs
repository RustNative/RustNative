//! Drag-and-drop into a window, through OLE's `IDropTarget`.
//!
//! Each top-level window registers one [`DropTarget`] COM object with
//! `RegisterDragDrop`. While something is dragged over the window, OLE
//! calls it on this (the UI) thread from inside its own message dispatch —
//! never while this window's `Runtime` is already borrowed, because the loop
//! holds no runtime borrow across `DispatchMessageW` (see
//! `message_loop::handle_message`). Each call resolves the runtime through
//! [`with_runtime`], exactly like a window procedure.
//!
//! The target node is found by walking *this window's own* child windows
//! under the drop point ([`deepest_child_at`]) rather than asking
//! `WindowFromPoint`, which would answer with whatever window is topmost on
//! the desktop: OLE has already established that the drop is over this
//! window, and the question here is only which of its nodes.
//!
//! A component accepts a drag by answering `DragEnter`/`DragOver` with
//! `InputRequests::set_drop_effect`; the answer is applied when that
//! dispatch finishes and read back here before returning to OLE.

use std::path::PathBuf;

use rustnative_core::{DragData, DropEffect, Event, NodeId, Point};
use windows::Win32::Foundation::POINTL;
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropTarget,
    RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;

use super::super::runtime::Runtime;
use super::super::win32::best_effort;
use super::pointer::{deepest_child_at, interested_ancestor};

/// `CF_UNICODETEXT`.
const CF_UNICODETEXT: u16 = 13;
/// `CF_HDROP`.
const CF_HDROP: u16 = 15;

/// One window's in-progress drag.
#[derive(Debug, Default)]
pub(crate) struct DragState {
    data: Option<DragData>,
    target: Option<NodeId>,
    /// The latest answer from the target's component.
    pub(crate) effect: DropEffect,
    /// The registered COM object, kept so the window can revoke it and so
    /// tests can drive it exactly as OLE does.
    pub(crate) registration: Option<IDropTarget>,
}

/// Registers `runtime`'s window as a drop target. Best effort: a thread
/// without OLE initialized (an application that initialized COM as
/// multithreaded before handing this thread to the framework) simply does
/// not accept drops.
pub(crate) fn register(runtime: &mut Runtime) {
    let target: IDropTarget = DropTarget { window: runtime.window as usize }.into();
    // SAFETY: `runtime.window` is this runtime's live top-level HWND, and
    // `target` is a valid `IDropTarget` OLE AddRefs for as long as the
    // registration lasts.
    let registered = unsafe { RegisterDragDrop(handle(runtime.window), &target) }.is_ok();
    best_effort(registered, "RegisterDragDrop", "the window does not accept drops");
    if registered {
        runtime.input.drag.registration = Some(target);
    }
}

/// Revokes the registration made by [`register`]; called from `WM_DESTROY`
/// while the window still exists, as `RevokeDragDrop` requires.
pub(crate) fn revoke(runtime: &mut Runtime) {
    if runtime.input.drag.registration.take().is_some() {
        // SAFETY: `runtime.window` is still live inside its own
        // `WM_DESTROY`, and was registered by `register`.
        let revoked = unsafe { RevokeDragDrop(handle(runtime.window)) }.is_ok();
        best_effort(revoked, "RevokeDragDrop", "OLE drops the registration with the window");
    }
}

fn handle(hwnd: HWND) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(hwnd)
}

fn to_native(effect: DropEffect) -> DROPEFFECT {
    match effect {
        DropEffect::Copy => DROPEFFECT_COPY,
        DropEffect::Move => DROPEFFECT_MOVE,
        DropEffect::Link => DROPEFFECT_LINK,
        DropEffect::None => DROPEFFECT_NONE,
    }
}

/// The effect to report to OLE: what the component asked for, if the drag
/// source allows it, otherwise nothing.
pub(crate) fn negotiate(requested: DropEffect, allowed: DROPEFFECT) -> DROPEFFECT {
    let wanted = to_native(requested);
    if wanted.0 & allowed.0 == 0 { DROPEFFECT_NONE } else { wanted }
}

/// Extracts the portable subset of a drag's data: files and plain text.
pub(crate) fn extract(data: &IDataObject) -> DragData {
    let mut drag = DragData::new();
    if let Some(files) = read_files(data) {
        drag = drag.with_files(files);
    }
    if let Some(text) = read_text(data) {
        drag = drag.with_text(text);
    }
    drag
}

fn format(format: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        // `TYMED_HGLOBAL` is a small positive flag.
        #[allow(clippy::cast_sign_loss)]
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn read_files(data: &IDataObject) -> Option<Vec<PathBuf>> {
    let request = format(CF_HDROP);
    // SAFETY: `request` is a fully initialized `FORMATETC` that outlives
    // the call; a failure (format not offered) is an `Err`.
    let mut medium = unsafe { data.GetData(&raw const request) }.ok()?;
    // SAFETY: a `TYMED_HGLOBAL` medium's union holds an `HGLOBAL`.
    let global = unsafe { medium.u.hGlobal };
    let hdrop = HDROP(global.0);
    // SAFETY: `hdrop` is the `CF_HDROP` global OLE just returned; index
    // `u32::MAX` with no buffer is the documented "how many files" query.
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
    let mut files = Vec::new();
    for index in 0..count {
        // SAFETY: as above, asking for one file's length in characters.
        let length = unsafe { DragQueryFileW(hdrop, index, None) };
        let mut buffer = vec![0u16; usize::try_from(length).unwrap_or(0) + 1];
        // SAFETY: `buffer` has room for `length` characters plus the NUL.
        let copied = unsafe { DragQueryFileW(hdrop, index, Some(&mut buffer)) };
        buffer.truncate(usize::try_from(copied).unwrap_or(0));
        files.push(PathBuf::from(String::from_utf16_lossy(&buffer)));
    }
    // SAFETY: releases the medium `GetData` handed over, exactly once.
    unsafe { ReleaseStgMedium(&raw mut medium) };
    Some(files)
}

fn read_text(data: &IDataObject) -> Option<String> {
    let request = format(CF_UNICODETEXT);
    // SAFETY: as in `read_files`.
    let mut medium = unsafe { data.GetData(&raw const request) }.ok()?;
    // SAFETY: as in `read_files`.
    let global = unsafe { medium.u.hGlobal };
    // SAFETY: `global` is the `CF_UNICODETEXT` global just returned.
    let pointer = unsafe { GlobalLock(global) }.cast::<u16>();
    let text = if pointer.is_null() {
        None
    } else {
        let mut length = 0usize;
        // SAFETY: `CF_UNICODETEXT` data is a NUL-terminated UTF-16 buffer,
        // locked (and so stable) until the unlock below.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: `length` initialized units were just walked above.
        let units = unsafe { std::slice::from_raw_parts(pointer, length) };
        let text = String::from_utf16_lossy(units);
        // SAFETY: pairs the successful lock above. Unlocking reports the
        // remaining lock count through its error channel, not a failure.
        let _ = unsafe { GlobalUnlock(global) };
        Some(text)
    };
    // SAFETY: as in `read_files`.
    unsafe { ReleaseStgMedium(&raw mut medium) };
    text
}

fn screen_point(pt: POINTL) -> POINT {
    POINT { x: pt.x, y: pt.y }
}

fn target_at(runtime: &Runtime, screen: POINT) -> Option<NodeId> {
    let under = deepest_child_at(runtime.window, screen);
    interested_ancestor(runtime, under, rustnative_core::InputInterest::wants_drop)
}

fn local(runtime: &Runtime, id: NodeId, screen: POINT) -> Point {
    let mut point = screen;
    if let Some(object) = runtime.renderer.registry.get(id) {
        // SAFETY: `object.hwnd()` is a live HWND from this window's
        // registry; `point` is a valid, exclusively borrowed `POINT`.
        let _ = unsafe { ScreenToClient(object.hwnd(), &raw mut point) };
    }
    Point::new(point.x, point.y)
}

/// Delivers a drag event to `target` and returns the component's answer.
fn ask(runtime: &mut Runtime, event: Event) -> DropEffect {
    runtime.input.drag.effect = DropEffect::None;
    if runtime.dispatch_or_quit(event) { runtime.input.drag.effect } else { DropEffect::None }
}

fn enter(runtime: &mut Runtime, data: DragData, screen: POINT) -> DropEffect {
    runtime.input.drag.data = Some(data.clone());
    runtime.input.drag.target = target_at(runtime, screen);
    match runtime.input.drag.target {
        Some(target) => {
            let position = local(runtime, target, screen);
            ask(runtime, Event::DragEnter { target, data, position })
        }
        None => DropEffect::None,
    }
}

fn over(runtime: &mut Runtime, screen: POINT) -> DropEffect {
    let data = runtime.input.drag.data.clone().unwrap_or_default();
    let next = target_at(runtime, screen);
    let previous = runtime.input.drag.target;
    if previous != next {
        runtime.input.drag.target = next;
        if let Some(previous) = previous {
            if !runtime.dispatch_or_quit(Event::DragLeave { target: previous }) {
                return DropEffect::None;
            }
        }
        return match next {
            Some(target) => {
                let position = local(runtime, target, screen);
                ask(runtime, Event::DragEnter { target, data, position })
            }
            None => DropEffect::None,
        };
    }
    match next {
        Some(target) => {
            let position = local(runtime, target, screen);
            ask(runtime, Event::DragOver { target, data, position })
        }
        None => DropEffect::None,
    }
}

fn leave(runtime: &mut Runtime) {
    let state = std::mem::take(&mut runtime.input.drag.data);
    let target = runtime.input.drag.target.take();
    runtime.input.drag.effect = DropEffect::None;
    if let (Some(target), Some(_)) = (target, state) {
        runtime.dispatch_or_quit(Event::DragLeave { target });
    }
}

fn drop_on(runtime: &mut Runtime, data: DragData, screen: POINT) -> DropEffect {
    // The last answer stands only if the drop lands on the node that gave
    // it; a drop point on a different node (possible if no `DragOver`
    // arrived for the final movement) is re-asked first.
    let accepted = if target_at(runtime, screen) == runtime.input.drag.target {
        runtime.input.drag.effect
    } else {
        over(runtime, screen)
    };
    let target = runtime.input.drag.target.take();
    runtime.input.drag.data = None;
    runtime.input.drag.effect = DropEffect::None;
    match target {
        Some(target) if accepted != DropEffect::None => {
            let position = local(runtime, target, screen);
            if runtime.dispatch_or_quit(Event::Drop { target, data, position }) {
                accepted
            } else {
                DropEffect::None
            }
        }
        _ => DropEffect::None,
    }
}

/// The OLE-facing COM object, in its own module so the lint exceptions
/// its generated code needs stay scoped to it.
#[allow(
    clippy::ref_as_ptr,
    clippy::inline_always,
    reason = "`windows::core::implement` generates this module's COM plumbing; the \
              `#[inline(always)]` accessors and reference-to-pointer casts are its, not ours"
)]
mod com {
    use rustnative_core::DropEffect;
    use windows::Win32::Foundation::POINTL;
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{DROPEFFECT, IDropTarget, IDropTarget_Impl};
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::core::{Ref, implement};
    use windows_sys::Win32::Foundation::HWND;

    use super::super::super::context::with_runtime;
    use super::super::super::runtime::Runtime;
    use super::{drop_on, enter, extract, leave, negotiate, over, screen_point};

    /// The COM object OLE calls. Holds only its window's handle (as an integer,
    /// since a COM object must not assume which thread drops it) and resolves
    /// everything else through the runtime on each call.
    // `Agile = false`: `#[implement]` objects are agile by default —
    // they answer `IAgileObject` and aggregate the free-threaded marshaler,
    // so COM hands callers in *other* apartments a direct pointer and they
    // call in on their own threads. This object reaches the window's
    // `Runtime`, which belongs to the UI thread alone, so it must be
    // marshaled into the UI thread's single-threaded apartment instead.
    #[implement(IDropTarget, Agile = false)]
    pub(crate) struct DropTarget {
        pub(crate) window: usize,
    }

    impl DropTarget {
        fn run(&self, f: impl FnOnce(&mut Runtime) -> DropEffect) -> DropEffect {
            with_runtime(self.window as HWND, f).unwrap_or(DropEffect::None)
        }
    }

    /// Writes `effect` through OLE's out-pointer, which carries the source's
    /// allowed effects on the way in.
    ///
    /// # Safety
    ///
    /// `slot` must be the valid `pdwEffect` pointer OLE passed to this call.
    unsafe fn answer(slot: *mut DROPEFFECT, effect: DropEffect) {
        if slot.is_null() {
            return;
        }
        // SAFETY: forwarded from this function's contract.
        unsafe { *slot = negotiate(effect, *slot) };
    }

    impl IDropTarget_Impl for DropTarget_Impl {
        fn DragEnter(
            &self,
            data: Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            let drag = data.as_ref().map(extract).unwrap_or_default();
            let chosen = self.run(|runtime| enter(runtime, drag, screen_point(*pt)));
            // SAFETY: `effect` is OLE's `pdwEffect` for this call.
            unsafe { answer(effect, chosen) };
            Ok(())
        }

        fn DragOver(
            &self,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            let chosen = self.run(|runtime| over(runtime, screen_point(*pt)));
            // SAFETY: as above.
            unsafe { answer(effect, chosen) };
            Ok(())
        }

        fn DragLeave(&self) -> windows::core::Result<()> {
            self.run(|runtime| {
                leave(runtime);
                DropEffect::None
            });
            Ok(())
        }

        fn Drop(
            &self,
            data: Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            let drag = data.as_ref().map(extract).unwrap_or_default();
            let chosen = self.run(|runtime| drop_on(runtime, drag, screen_point(*pt)));
            // SAFETY: as above.
            unsafe { answer(effect, chosen) };
            Ok(())
        }
    }
}

pub(crate) use com::DropTarget;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_only_grants_what_the_source_allows() {
        let copy_or_move = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0);
        assert_eq!(negotiate(DropEffect::Move, copy_or_move), DROPEFFECT_MOVE);
        assert_eq!(negotiate(DropEffect::Link, copy_or_move), DROPEFFECT_NONE);
        assert_eq!(negotiate(DropEffect::None, copy_or_move), DROPEFFECT_NONE);
    }
}
