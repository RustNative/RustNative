//! Realizes a portable `MenuBar` as a native Win32 menu.

use std::collections::HashMap;

use rustnative_core::{MenuBar, MenuItem, NodeId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateMenu, CreatePopupMenu, DestroyMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING,
};

use super::util::wide;
use crate::Error;
use crate::error::NativeContext;

/// A native menu realized from a portable `MenuBar`, together with the
/// command-id-to-`NodeId` table `window_proc` uses to translate a
/// `WM_COMMAND` menu selection back into an `Event::MenuAction`.
pub(crate) struct BuiltMenu {
    menu: OwnedMenu,
    commands: HashMap<u16, NodeId>,
}

impl std::fmt::Debug for BuiltMenu {
    /// Reports how many commands were registered, never the raw `HMENU`.
    /// A handle value is a process-local allocation-table index — it means
    /// nothing to a reader and is exactly the kind of detail `Display`/
    /// `Debug` should not surface (see `crate::error`'s module docs on the
    /// same rule for native handles in errors).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltMenu")
            .field("commands", &self.commands.len())
            .field("attached", &self.menu.attached)
            .finish()
    }
}

/// Owns a menu until it is transferred to a top-level HWND. This makes
/// all menu-construction failure paths release nested Win32 resources.
struct OwnedMenu {
    handle: HMENU,
    attached: bool,
}

impl OwnedMenu {
    fn new(handle: HMENU) -> Self {
        Self { handle, attached: false }
    }
}

impl Drop for OwnedMenu {
    fn drop(&mut self) {
        if !self.attached && !self.handle.is_null() {
            // SAFETY: `self.handle` was just checked non-null, and
            // `self.attached` being false means ownership was never
            // transferred to a window via `SetMenu` (see
            // `into_attached`), so this `OwnedMenu` is still the sole
            // owner responsible for destroying it — exactly once, since
            // `Drop::drop` runs at most once.
            unsafe {
                DestroyMenu(self.handle);
            }
        }
    }
}

impl BuiltMenu {
    pub(crate) fn handle(&self) -> HMENU {
        self.menu.handle
    }

    pub(crate) fn into_attached(mut self) -> (HMENU, HashMap<u16, NodeId>) {
        self.menu.attached = true;
        (self.menu.handle, std::mem::take(&mut self.commands))
    }
}

pub(crate) fn build_native_menu(menu: &MenuBar) -> Result<BuiltMenu, Error> {
    // SAFETY: `CreateMenu` takes no arguments; a null return (checked
    // below) is its documented failure signal.
    let handle = unsafe { CreateMenu() };
    if handle.is_null() {
        return Err(Error::windows_api("CreateMenu"));
    }
    // Take ownership *before* the first fallible call below, not after the
    // loop succeeds. An earlier revision constructed the `OwnedMenu` only
    // in the success path, so a failure inside `append_menu_item` — an
    // out-of-resources `CreatePopupMenu`, a failed `AppendMenuW`, or
    // `Error::MenuCommandExhausted` — returned through `?` while `handle`
    // was still a bare `HMENU` owned by nothing, leaking the whole
    // partially built menu tree. That is precisely the failure mode the
    // standards audit's P1.14 finding calls out ("on intermediate failure,
    // the code returns an error without a clear ownership guard"), and it
    // is why ownership is established here rather than at the end.
    let owned = OwnedMenu::new(handle);
    let mut commands = HashMap::new();
    let mut next_command_id: u16 = 1;
    for item in menu.items() {
        append_menu_item(handle, item, &mut commands, &mut next_command_id)?;
    }
    Ok(BuiltMenu { menu: owned, commands })
}

fn append_menu_item(
    parent: HMENU,
    item: &MenuItem,
    commands: &mut HashMap<u16, NodeId>,
    next_command_id: &mut u16,
) -> Result<(), Error> {
    if item.is_separator() {
        // SAFETY: `parent` is a live `HMENU` owned by the caller
        // (either freshly created by `build_native_menu` or an ancestor
        // `submenu` created below, in either case not yet attached to a
        // window); `MF_SEPARATOR` ignores the `uIDNewItem`/`lpNewItem`
        // arguments, so the null `lpNewItem` here is valid.
        if unsafe { AppendMenuW(parent, MF_SEPARATOR, 0, std::ptr::null()) } == 0 {
            return Err(Error::windows_api("AppendMenuW(separator)"));
        }
        return Ok(());
    }

    let label = wide(item.label());
    if item.is_submenu() {
        // SAFETY: `CreatePopupMenu` takes no arguments; a null return
        // (checked below) is its documented failure signal.
        let submenu = unsafe { CreatePopupMenu() };
        if submenu.is_null() {
            return Err(Error::windows_api("CreatePopupMenu"));
        }
        let submenu_guard = OwnedMenu::new(submenu);
        for child in item.children() {
            append_menu_item(submenu, child, commands, next_command_id)?;
        }
        let mut flags = MF_STRING | MF_POPUP;
        if !item.is_enabled() {
            flags |= MF_GRAYED;
        }
        // SAFETY: `parent` is a live `HMENU` as above; `submenu` was
        // just checked non-null and, per `MF_POPUP`, is consumed as a
        // submenu handle rather than a plain item id; `label` is a
        // NUL-terminated wide buffer kept alive for the duration of
        // this call.
        if unsafe { AppendMenuW(parent, flags, submenu as usize, label.as_ptr()) } == 0 {
            return Err(Error::windows_api("AppendMenuW(submenu)"));
        }
        // The parent menu now owns the submenu and will destroy it.
        std::mem::forget(submenu_guard);
    } else {
        let command_id = *next_command_id;
        *next_command_id = next_command_id
            .checked_add(1)
            // The window is filled in by `create_window_once`, which is the
            // frame that knows which window's menu this is; see
            // `Error::or_context`.
            .ok_or(Error::MenuCommandExhausted { context: NativeContext::none() })?;
        commands.insert(command_id, item.id());

        let mut flags = MF_STRING;
        if !item.is_enabled() {
            flags |= MF_GRAYED;
        }
        if item.is_checked() == Some(true) {
            flags |= MF_CHECKED;
        }
        // SAFETY: `parent` is a live `HMENU` owned by the caller, as in
        // the branches above; `label` is a NUL-terminated wide buffer
        // kept alive for the duration of this call; `command_id`, per
        // plain `MF_STRING`, is consumed as a numeric item id, not a
        // pointer.
        if unsafe { AppendMenuW(parent, flags, command_id as usize, label.as_ptr()) } == 0 {
            return Err(Error::windows_api("AppendMenuW(item)"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustnative_core::MenuBar;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMenuItemCount, IsMenu};

    fn menu_bar() -> MenuBar {
        MenuBar::new([
            MenuItem::submenu(
                "file",
                "File",
                [
                    MenuItem::action("file.open", "Open"),
                    MenuItem::separator(),
                    MenuItem::action("file.quit", "Quit"),
                ],
            ),
            MenuItem::submenu("edit", "Edit", [MenuItem::action("edit.copy", "Copy")]),
        ])
    }

    #[test]
    fn building_a_menu_assigns_one_command_id_per_selectable_item() {
        let built = build_native_menu(&menu_bar()).expect("building a small menu must succeed");
        // SAFETY: `handle()` is a live `HMENU` this `BuiltMenu` owns;
        // `IsMenu`/`GetMenuItemCount` accept any handle value and report on
        // it rather than dereferencing it as a pointer.
        assert!(unsafe { IsMenu(built.handle()) } != 0, "the built handle must be a real HMENU");
        assert_eq!(
            // SAFETY: as above — a live `HMENU` this `BuiltMenu` owns.
            unsafe { GetMenuItemCount(built.handle()) },
            2,
            "the bar itself holds the two submenus"
        );

        let commands = &built.commands;
        // Three selectable items across both submenus; separators and
        // submenu headers get no command id, since neither is selectable.
        assert_eq!(commands.len(), 3);
        let mut ids: Vec<_> = commands.keys().copied().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2, 3], "ids are allocated densely from 1");

        let mut targets: Vec<_> = commands.values().copied().collect();
        targets.sort_unstable_by_key(|id| id.get());
        let mut expected = vec![
            rustnative_core::NodeId::from_key("file.open"),
            rustnative_core::NodeId::from_key("file.quit"),
            rustnative_core::NodeId::from_key("edit.copy"),
        ];
        expected.sort_unstable_by_key(|id| id.get());
        assert_eq!(targets, expected, "every id maps back to the item that declared it");
    }

    #[test]
    fn an_unattached_menu_is_destroyed_when_it_is_dropped() {
        let handle = {
            let built = build_native_menu(&menu_bar()).expect("building must succeed");
            built.handle()
        };
        // SAFETY: `handle` may now be a stale value; `IsMenu` is documented
        // to accept any handle and answer whether it currently names a menu,
        // which is exactly the question here.
        assert!(
            // SAFETY: `handle` may now be stale; `IsMenu` is documented to
            // accept any handle value and answer whether it currently names
            // a menu, which is exactly the question here.
            unsafe { IsMenu(handle) } == 0,
            "dropping a `BuiltMenu` that was never attached to a window must destroy its HMENU"
        );
    }

    #[test]
    fn an_attached_menu_is_left_alone_for_the_window_to_own() {
        let built = build_native_menu(&menu_bar()).expect("building must succeed");
        let (handle, commands) = built.into_attached();
        assert_eq!(commands.len(), 3, "the command table transfers with ownership");
        // SAFETY: as above.
        assert!(
            // SAFETY: as in the test above.
            unsafe { IsMenu(handle) } != 0,
            "ownership transferred to the window, so the guard must not have destroyed it"
        );
        // Nothing owns it now that the test has taken it out of the guard,
        // so clean up rather than leaking it for the rest of the run.
        // SAFETY: `handle` is a live `HMENU` that `into_attached` released
        // ownership of, and nothing else holds it.
        unsafe { DestroyMenu(handle) };
    }

    /// The process's live USER-object count, which on Windows tracks menu
    /// handles exactly: creating N menus raises it by N, destroying them
    /// returns it to where it started.
    ///
    /// This is deliberately *not* the `GetGuiResources(GR_GDIOBJECTS)`
    /// reading that an earlier pass found unreliable for brushes and fonts
    /// (see `rendering::styling`'s own leak test for that story).
    /// `GR_USEROBJECTS` was verified against real menu handles before being
    /// relied on here, and it moves one-for-one.
    fn live_user_objects() -> u32 {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetGuiResources(process: isize, flags: u32) -> u32;
            fn GetCurrentProcess() -> isize;
        }
        /// `GR_USEROBJECTS` — count USER objects (windows, menus, cursors)
        /// rather than GDI ones.
        const GR_USEROBJECTS: u32 = 1;
        // SAFETY: both functions take plain values and no pointer
        // arguments; `GetCurrentProcess` returns a pseudo-handle that needs
        // no closing.
        unsafe { GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS) }
    }

    #[test]
    fn a_failure_partway_through_building_does_not_leak_the_partial_menu() {
        // The regression this pins down: `build_native_menu` used to wrap
        // its `CreateMenu` handle in an `OwnedMenu` only on the success
        // path, so a failure inside `append_menu_item` returned through `?`
        // while the bar it had already created was owned by nothing
        // (standards audit P1.14). One leaked `HMENU` per failed build —
        // invisible in any single call, fatal to a long-running application
        // that rebuilds menus.
        //
        // Provoking the failure: Windows caps a single menu at roughly 1,170
        // items and then fails `AppendMenuW`, well before this crate's `u16`
        // command-id space runs out. Worth knowing on its own —
        // `Error::MenuCommandExhausted` is a defensive guard, not the
        // failure a real application meets first.
        const ITEMS: u32 = 1_500;
        const ATTEMPTS: u32 = 64;
        // The USER-object count is process-wide; window tests running
        // concurrently (the UI Automation ones create and release windows
        // on their own threads) would move it for reasons unrelated to
        // menus.
        let _exclusive = crate::native::harness::exclusive();

        let items = (0..ITEMS)
            .map(|index| MenuItem::action(format!("item-{index}"), "x"))
            .collect::<Vec<_>>();
        let oversized = MenuBar::new([MenuItem::submenu("root", "Root", items)]);

        // One warm-up attempt first: the very first failed build can move
        // the count for reasons unrelated to this crate (a lazily created
        // window station object, say), so the baseline is taken after the
        // code path has run once rather than before.
        let _ = build_native_menu(&oversized);
        let baseline = live_user_objects();

        for attempt in 0..ATTEMPTS {
            let error = build_native_menu(&oversized)
                .expect_err("a menu past Windows' own item limit must fail while being built");
            assert!(
                matches!(error, Error::MenuCommandExhausted { .. } | Error::WindowsApi { .. }),
                "attempt {attempt} failed with an unexpected error: {error:?}"
            );
        }

        let leaked = live_user_objects().saturating_sub(baseline);
        assert_eq!(
            leaked, 0,
            "{ATTEMPTS} failed menu builds leaked {leaked} USER handles; every failure path in \
             `build_native_menu` must return its handles (P1.14)"
        );
    }
}
