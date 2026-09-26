//! Tab bars, realized as the system's own `SysTabControl32`.
//!
//! Using the system control rather than drawing tabs is the point: it looks
//! like every other tab strip on the machine, follows the system theme, and
//! comes with a UI Automation tab-list provider whose tab items Narrator
//! already knows how to announce and select.
//!
//! The control is *controlled* in the declarative sense. Clicking a tab
//! changes the control's selection immediately — Windows does that — and
//! raises `TCN_SELCHANGE`, which becomes `Event::TabSelected`; after the
//! component has answered, the renderer puts the control back to whatever
//! the node now says. A component that accepts the choice renders it, and
//! one that refuses it (an unsaved-changes guard, say) keeps the old tab
//! selected without the control disagreeing with it.

use std::sync::Once;

use rustnative_core::Tabs;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::{
    ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TCIF_TEXT, TCITEMW,
    TCM_DELETEALLITEMS, TCM_GETCURSEL, TCM_INSERTITEMW, TCM_SETCURSEL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

use super::super::util::wide;
use super::super::win32::{best_effort, informational};

/// The system tab control's class name.
pub(crate) const TAB_CLASS_NAME: &str = "SysTabControl32";

/// Registers the common-controls tab class for this process, once.
pub(crate) fn ensure_class() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let init = INITCOMMONCONTROLSEX {
            dwSize: u32::try_from(size_of::<INITCOMMONCONTROLSEX>()).unwrap_or(u32::MAX),
            dwICC: ICC_TAB_CLASSES,
        };
        // SAFETY: `init` is fully initialized and borrowed for the call.
        let registered = unsafe { InitCommonControlsEx(&raw const init) } != 0;
        best_effort(registered, "InitCommonControlsEx(ICC_TAB_CLASSES)", "tab bars fail to create");
    });
}

/// Makes the control show `tabs`: its labels, in order, and its selection.
pub(crate) fn sync(hwnd: HWND, tabs: &Tabs) {
    // SAFETY: `hwnd` is a live tab control; this message takes no pointers.
    informational(unsafe { SendMessageW(hwnd, TCM_DELETEALLITEMS, 0, 0) });
    for (index, label) in tabs.labels().iter().enumerate() {
        let mut text = wide(label);
        let item = TCITEMW { mask: TCIF_TEXT, pszText: text.as_mut_ptr(), ..TCITEMW::default() };
        // SAFETY: `item` and the text it points to live for the call; the
        // control copies the text.
        let inserted =
            unsafe { SendMessageW(hwnd, TCM_INSERTITEMW, index, (&raw const item) as isize) };
        best_effort(inserted >= 0, "TCM_INSERTITEMW", "the tab bar is missing a tab");
    }
    select(hwnd, tabs.selected());
}

/// Selects tab `index` without raising a selection notification.
pub(crate) fn select(hwnd: HWND, index: usize) {
    // SAFETY: `hwnd` is a live tab control; `TCM_SETCURSEL` takes an index
    // and returns the previous one.
    informational(unsafe { SendMessageW(hwnd, TCM_SETCURSEL, index, 0) });
}

/// The tab the control currently has selected, if any.
pub(crate) fn selection(hwnd: HWND) -> Option<usize> {
    // SAFETY: `hwnd` is a live tab control; this message takes no pointers.
    usize::try_from(unsafe { SendMessageW(hwnd, TCM_GETCURSEL, 0, 0) }).ok()
}
