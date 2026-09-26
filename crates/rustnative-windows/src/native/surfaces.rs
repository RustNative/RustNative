//! Surfaces beyond the main window (`PLAN.md` Milestone 57): the tray
//! icon and its menu (`Shell_NotifyIconW`), its notifications (`NIF_INFO`,
//! which Windows 10 and 11 show as toasts), the jump list
//! (`ICustomDestinationList`), and taskbar progress (`ITaskbarList3`).
//!
//! What the application asks through `rustnative_core::surfaces::Surfaces`
//! is applied after each change it makes; the tray icon belongs to the
//! primary window, whose `WNDPROC` receives the icon's callback message
//! and turns it into `Event::SurfaceAction`.

use rustnative_core::capability::SurfaceKind;
use rustnative_core::surfaces::{ACTIVATE, JumpTask, NOTIFICATION, SurfaceCommand, TrayMenuItem};
use rustnative_core::{Event, WindowId};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Shell::Common::IObjectCollection;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW,
    ITaskbarList3, ShellLink, TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList,
};
use windows_core::{HSTRING, Interface, PCWSTR};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NIN_BALLOONUSERCLICK, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, IDI_APPLICATION, LoadIconW, MF_STRING,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP,
    WM_CONTEXTMENU, WM_LBUTTONUP,
};

use super::runtime::Runtime;
use super::util::wide;

/// The tray icon's callback message to the primary window.
pub(crate) const WM_FRAMEWORK_TRAY: u32 = WM_APP + 8;

/// The icon's id within the window.
const TRAY_ID: u32 = 1;

/// `PKEY_Title` (`System.Title`): a jump-list task's label.
const PKEY_TITLE: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows_core::GUID::from_u128(0xf29f_85e0_4ff9_1068_ab91_0800_2b27_b3d9),
    pid: 2,
};

/// The tray's state on the primary window's runtime.
#[derive(Debug, Default)]
pub(crate) struct Tray {
    /// Whether the icon is added.
    pub(crate) shown: bool,
    /// Its menu.
    pub(crate) menu: Vec<TrayMenuItem>,
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    // SAFETY: zeroed is a valid NOTIFYICONDATAW before its fields are set.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(0);
    data.hWnd = hwnd;
    data.uID = TRAY_ID;
    data
}

fn copy(text: &str, into: &mut [u16]) {
    let wide: Vec<u16> = text.encode_utf16().take(into.len().saturating_sub(1)).collect();
    into[..wide.len()].copy_from_slice(&wide);
    into[wide.len()] = 0;
}

fn show_tray(runtime: &mut Runtime, tooltip: &str, menu: Vec<TrayMenuItem>) -> bool {
    let mut data = icon_data(runtime.window);
    data.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
    data.uCallbackMessage = WM_FRAMEWORK_TRAY;
    // SAFETY: a stock icon, shared and never freed.
    data.hIcon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
    copy(tooltip, &mut data.szTip);
    let action = if runtime.tray.shown { NIM_MODIFY } else { NIM_ADD };
    // SAFETY: a fully initialized NOTIFYICONDATAW for this window's icon.
    let ok = unsafe { Shell_NotifyIconW(action, &raw const data) } != 0;
    if ok && !runtime.tray.shown {
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: as above; opts into the version-4 callback layout.
        unsafe { Shell_NotifyIconW(NIM_SETVERSION, &raw const data) };
    }
    runtime.tray.shown |= ok;
    runtime.tray.menu = menu;
    ok
}

fn hide_tray(runtime: &mut Runtime) {
    if runtime.tray.shown {
        let data = icon_data(runtime.window);
        // SAFETY: removing this window's own icon.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
        runtime.tray.shown = false;
    }
}

fn notify(runtime: &mut Runtime, title: &str, body: &str) -> bool {
    if !runtime.tray.shown && !show_tray(runtime, title, Vec::new()) {
        return false;
    }
    let mut data = icon_data(runtime.window);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_INFO;
    copy(title, &mut data.szInfoTitle);
    copy(body, &mut data.szInfo);
    // SAFETY: as above.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) != 0 }
}

/// Replaces the jump list's tasks.
pub(crate) fn set_jump_list(tasks: &[JumpTask]) -> windows_core::Result<()> {
    let exe = std::env::current_exe().map_err(|error| {
        windows_core::Error::new(windows_core::HRESULT(-2_147_467_259), error.to_string())
    })?;
    let exe = HSTRING::from(exe.to_string_lossy().as_ref());
    // SAFETY: COM is initialized on this (the UI) thread by the backend;
    // every interface below is owned and released on drop.
    unsafe {
        let list: ICustomDestinationList =
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
        let mut slots = 0u32;
        let _removed: windows::Win32::UI::Shell::Common::IObjectArray =
            list.BeginList(&raw mut slots)?;
        let collection: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        for task in tasks {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(PCWSTR(exe.as_ptr()))?;
            let arguments = HSTRING::from(task.arguments.as_str());
            link.SetArguments(PCWSTR(arguments.as_ptr()))?;
            let store: IPropertyStore = link.cast()?;
            let title = PROPVARIANT::from(task.label.as_str());
            let key = PKEY_TITLE;
            store.SetValue(&raw const key, &raw const title)?;
            store.Commit()?;
            collection.AddObject(&link)?;
        }
        // An empty list is committed without tasks: `AddUserTasks` refuses
        // an empty collection.
        if !tasks.is_empty() {
            list.AddUserTasks(
                &collection.cast::<windows::Win32::UI::Shell::Common::IObjectArray>()?,
            )?;
        }
        list.CommitList()
    }
}

/// Shows (0–1) or removes progress on `hwnd`'s taskbar button.
pub(crate) fn set_progress(hwnd: HWND, progress: Option<f32>) -> windows_core::Result<()> {
    let hwnd = windows::Win32::Foundation::HWND(hwnd);
    // SAFETY: as in `set_jump_list`; `hwnd` is a live top-level window.
    unsafe {
        let taskbar: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
        taskbar.HrInit()?;
        match progress {
            Some(value) => {
                // Hundredths: the precision a taskbar button can show.
                let done = (value.clamp(0.0, 1.0) * 100.0).round();
                taskbar.SetProgressState(hwnd, TBPF_NORMAL)?;
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "0–100 after the clamp"
                )]
                taskbar.SetProgressValue(hwnd, done as u64, 100)
            }
            None => taskbar.SetProgressState(hwnd, TBPF_NOPROGRESS),
        }
    }
}

/// Applies what the application asked of its surfaces since the last call.
pub(crate) fn apply(runtime: &mut Runtime) {
    if runtime.window_id != WindowId::PRIMARY {
        return;
    }
    let commands = runtime.with_application(|application| application.services().surfaces().take());
    for command in commands {
        match command {
            SurfaceCommand::ShowTray { tooltip, menu } => {
                show_tray(runtime, &tooltip, menu);
            }
            SurfaceCommand::HideTray => hide_tray(runtime),
            SurfaceCommand::JumpList(tasks) => {
                let _ = set_jump_list(&tasks);
            }
            SurfaceCommand::Progress(progress) => {
                let _ = set_progress(runtime.window, progress);
            }
            SurfaceCommand::Notify { title, body } => {
                notify(runtime, &title, &body);
            }
            _ => {}
        }
    }
}

/// The action for the tray icon's callback `event` (version 4: the event is
/// the low word of `lParam`), showing the menu when it asks for it.
pub(crate) fn tray_callback(runtime: &mut Runtime, event: u32) {
    let action = match event {
        WM_LBUTTONUP => Some(ACTIVATE.to_owned()),
        NIN_BALLOONUSERCLICK => Some(NOTIFICATION.to_owned()),
        WM_CONTEXTMENU => choose_from_menu(runtime),
        _ => None,
    };
    if let Some(action) = action {
        dispatch_action(runtime, action);
    }
}

/// Delivers a tray action as `Event::SurfaceAction`.
pub(crate) fn dispatch_action(runtime: &mut Runtime, action: String) {
    let window = runtime.window_id;
    runtime.dispatch_or_quit(Event::SurfaceAction {
        window,
        surface: SurfaceKind::TrayExtra,
        action,
    });
}

fn choose_from_menu(runtime: &Runtime) -> Option<String> {
    if runtime.tray.menu.is_empty() {
        return None;
    }
    // SAFETY: a new menu, destroyed below.
    let menu = unsafe { CreatePopupMenu() };
    for (index, item) in runtime.tray.menu.iter().enumerate() {
        let label = wide(&item.label);
        // SAFETY: a live menu and a NUL-terminated label.
        unsafe { AppendMenuW(menu, MF_STRING, index + 1, label.as_ptr()) };
    }
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: an out-pointer; the window is made foreground so the menu
    // closes when the person clicks elsewhere (documented requirement).
    let chosen = unsafe {
        GetCursorPos(&raw mut point);
        SetForegroundWindow(runtime.window);
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            runtime.window,
            std::ptr::null(),
        )
    };
    // SAFETY: the menu created above.
    unsafe { DestroyMenu(menu) };
    let index = usize::try_from(chosen).ok()?.checked_sub(1)?;
    runtime.tray.menu.get(index).map(|item| item.id.clone())
}

/// Removes the icon when the window goes.
pub(crate) fn teardown(runtime: &mut Runtime) {
    hide_tray(runtime);
}
