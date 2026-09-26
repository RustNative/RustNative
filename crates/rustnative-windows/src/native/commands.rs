//! The command model (`rustnative_core::command`), realized on Windows.
//!
//! - **Shortcuts** are offered to the application's commands before a key
//!   press becomes a `KeyDown` event, so a command's shortcut works from
//!   anywhere in the window — the job an accelerator table does, done here
//!   against the live declarations so a shortcut can never outlive, or
//!   disagree with, the command it belongs to.
//! - **Menu items** bound to a command are brought up to date as their
//!   popup opens (`WM_INITMENUPOPUP`): enabled or greyed, checked or not,
//!   and labelled with the shortcut that works — `Save\tCtrl+S`, the tab
//!   being Windows' own convention for right-aligning it.

use rustnative_core::{KeyCode, KeyModifiers, NodeId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CheckMenuItem, EnableMenuItem, HMENU, MENUITEMINFOW, MF_BYCOMMAND, MF_CHECKED, MF_ENABLED,
    MF_GRAYED, MF_UNCHECKED, MIIM_STRING, SetMenuItemInfoW,
};

use super::runtime::Runtime;
use super::util::wide;
use super::win32::ignored_by_contract;
use crate::Error;

/// Offers a key press to the window's command shortcuts. Returns whether a
/// command took it, in which case it must not be delivered further.
pub(crate) fn shortcut(
    runtime: &mut Runtime,
    key: KeyCode,
    modifiers: KeyModifiers,
    focused: Option<NodeId>,
) -> Result<bool, Error> {
    let window = runtime.window_id;
    let handled = runtime.with_application(|application| {
        application.handle_shortcut(window, key, modifiers, focused)
    });
    if handled {
        runtime.finish_application_change(true)?;
    }
    Ok(handled)
}

/// Brings every command-bound item of the popup `menu` up to date.
pub(crate) fn init_menu_popup(runtime: &Runtime, menu: HMENU, focused: Option<NodeId>) {
    let window = runtime.window_id;
    for (command_id, item) in &runtime.menu_commands {
        let state = runtime.with_application(|application| {
            let bound = application
                .window_for(window)
                .and_then(|window| window.menu())
                .and_then(|bar| bar.find(*item))
                .map(|item| (item.bound_command(), item.label().to_owned()));
            let Some((Some(command), label)) = bound else { return None };
            let declared = application.command_state(window, command, focused);
            Some((label, declared))
        });
        let Some((label, declared)) = state else { continue };
        let (enabled, checked, shortcut) =
            declared.as_ref().map_or((false, None, None), |command| {
                (command.is_enabled(), command.is_checked(), command.shortcut_label())
            });
        let id = u32::from(*command_id);
        // SAFETY: `menu` is the live popup Windows is about to show; an id
        // that is not in this popup makes these calls fail harmlessly
        // (they return -1), which is why their results are not checked.
        unsafe {
            ignored_by_contract(EnableMenuItem(
                menu,
                id,
                MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED },
            ));
            if let Some(checked) = checked {
                ignored_by_contract(CheckMenuItem(
                    menu,
                    id,
                    MF_BYCOMMAND | if checked { MF_CHECKED } else { MF_UNCHECKED },
                ));
            }
        }
        let text =
            shortcut.map_or_else(|| label.clone(), |shortcut| format!("{label}\t{shortcut}"));
        let mut text = wide(text);
        let info = MENUITEMINFOW {
            cbSize: u32::try_from(std::mem::size_of::<MENUITEMINFOW>()).unwrap_or(80),
            fMask: MIIM_STRING,
            dwTypeData: text.as_mut_ptr(),
            ..MENUITEMINFOW::default()
        };
        // SAFETY: `info.cbSize` is set, `MIIM_STRING` names the one field
        // read, and `text` is a NUL-terminated buffer that outlives the call.
        ignored_by_contract(unsafe { SetMenuItemInfoW(menu, id, 0, &raw const info) });
    }
}
