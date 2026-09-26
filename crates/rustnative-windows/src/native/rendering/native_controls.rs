//! The native controls of `rustnative_core::control` (`PLAN.md` Milestone
//! 48), realized as the system's own: `BUTTON` check boxes and radio
//! buttons, the common-controls trackbar, progress bar, date picker,
//! up-down, and link, `COMBOBOX`, `LISTBOX`, multi-line `EDIT`, an etched
//! `STATIC` rule, and this crate's canvas for images.
//!
//! Each control is *controlled*: the person's change raises an event, and
//! after the component has answered, [`sync`] puts the control back to what
//! the node says. None of the setters used here raises a notification of
//! its own except `EDIT`'s, which the renderer already suppresses.
//!
//! Windows has no switch in Win32; a [`Control::Toggle`] is a check box
//! (`docs/idioms/windows.md`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::sync::Once;

use rustnative_core::{CalendarDate, Control, DrawList, RectF};
use windows_sys::Win32::Foundation::{HWND, SYSTEMTIME};
use windows_sys::Win32::System::SystemServices::{SS_ETCHEDHORZ, SS_NOTIFY};
use windows_sys::Win32::UI::Controls::{
    DTM_GETSYSTEMTIME, DTM_SETSYSTEMTIME, GDT_VALID, ICC_BAR_CLASSES, ICC_DATE_CLASSES,
    ICC_LINK_CLASS, ICC_PROGRESS_CLASS, ICC_UPDOWN_CLASS, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, PBM_SETMARQUEE, PBM_SETPOS, PBS_MARQUEE, PBS_SMOOTH, TBM_SETPOS,
    TBM_SETRANGEMAX, TBM_SETRANGEMIN, TBS_HORZ, TBS_NOTICKS, UDM_SETBUDDY, UDM_SETRANGE32,
    UDS_ALIGNRIGHT, UDS_ARROWKEYS, UDS_NOTHOUSANDS, UDS_SETBUDDYINT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_CHECKBOX, BS_RADIOBUTTON, CB_ADDSTRING, CB_GETCURSEL,
    CB_RESETCONTENT, CB_SETCURSEL, CBS_DROPDOWNLIST, CreateWindowExW, ES_AUTOVSCROLL, ES_LEFT,
    ES_MULTILINE, ES_NUMBER, ES_WANTRETURN, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT,
    LB_SETCURSEL, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, SendMessageW, SetWindowTextW, WS_BORDER,
    WS_CHILD, WS_VISIBLE, WS_VSCROLL,
};

use super::super::util::{module_instance, wide, window_text};
use super::super::win32::{best_effort, informational};

/// `TBM_GETPOS` (`WM_USER`), which `windows-sys` does not name.
const TBM_GETPOS: u32 = 0x0400;

/// Which native control realizes a control: a stable tag, so a node whose
/// control changes kind gets a new window.
#[must_use]
pub(crate) fn tag(control: &Control) -> &'static str {
    match control {
        Control::Checkbox { .. } | Control::Toggle { .. } => "check",
        Control::Radio { .. } => "radio",
        Control::Slider { .. } => "trackbar",
        Control::Progress { percent: Some(_) } => "progress",
        Control::Progress { percent: None } => "progress-marquee",
        Control::Select { .. } => "combobox",
        Control::ListBox { .. } => "listbox",
        Control::DatePicker { .. } => "datetime",
        Control::Spinner { .. } => "spinner",
        Control::Separator => "separator",
        Control::Link { .. } => "link",
        Control::MultilineText { .. } => "multiline",
        Control::Image { .. } => "image",
        _ => "unknown",
    }
}

/// Registers the common-control classes these need, once.
fn ensure_classes() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let size = u32::try_from(size_of::<INITCOMMONCONTROLSEX>()).unwrap_or(u32::MAX);
        for classes in [ICC_BAR_CLASSES, ICC_DATE_CLASSES, ICC_PROGRESS_CLASS, ICC_UPDOWN_CLASS] {
            let init = INITCOMMONCONTROLSEX { dwSize: size, dwICC: classes };
            // SAFETY: `init` is fully initialized and borrowed for the call.
            let registered = unsafe { InitCommonControlsEx(&raw const init) } != 0;
            best_effort(registered, "InitCommonControlsEx(controls)", "a control fails to create");
        }
        // `SysLink` exists only in Common Controls 6, which an application
        // gets from its manifest (`rustnative_build`); without it a link
        // falls back to clickable text (see `create`).
        let link = INITCOMMONCONTROLSEX { dwSize: size, dwICC: ICC_LINK_CLASS };
        // SAFETY: as above.
        let _ = unsafe { InitCommonControlsEx(&raw const link) };
    });
}

#[allow(clippy::cast_sign_loss, reason = "the Win32 style constants are small positive flags")]
fn style(value: i32) -> u32 {
    value as u32
}

/// The window class, creation text, and styles realizing `control`.
fn class_of(control: &Control) -> (&'static str, String, u32) {
    match control {
        Control::Checkbox { label, .. } | Control::Toggle { label, .. } => {
            ("BUTTON", label.clone(), style(BS_CHECKBOX))
        }
        Control::Radio { label, .. } => ("BUTTON", label.clone(), style(BS_RADIOBUTTON)),
        Control::Slider { .. } => ("msctls_trackbar32", String::new(), TBS_HORZ | TBS_NOTICKS),
        Control::Progress { percent } => (
            "msctls_progress32",
            String::new(),
            if percent.is_some() { PBS_SMOOTH } else { PBS_MARQUEE },
        ),
        Control::Select { .. } => ("COMBOBOX", String::new(), style(CBS_DROPDOWNLIST) | WS_VSCROLL),
        Control::ListBox { .. } => (
            "LISTBOX",
            String::new(),
            style(LBS_NOTIFY) | style(LBS_NOINTEGRALHEIGHT) | WS_VSCROLL | WS_BORDER,
        ),
        Control::DatePicker { .. } => ("SysDateTimePick32", String::new(), 0),
        Control::Spinner { value, .. } => {
            ("EDIT", value.to_string(), WS_BORDER | style(ES_NUMBER) | style(ES_LEFT))
        }
        Control::Separator => ("STATIC", String::new(), SS_ETCHEDHORZ),
        Control::Link { text } => ("SysLink", format!("<a>{}</a>", escape_link(text)), 0),
        Control::MultilineText { value } => (
            "EDIT",
            value.clone(),
            WS_BORDER
                | WS_VSCROLL
                | style(ES_MULTILINE)
                | style(ES_AUTOVSCROLL)
                | style(ES_WANTRETURN),
        ),
        // Images are drawn by this crate's canvas (see `create`); this arm
        // is never reached for them.
        _ => ("STATIC", String::new(), 0),
    }
}

/// A link's text with the markup characters `SysLink` reads escaped.
fn escape_link(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The draw list showing an image at its natural size.
#[must_use]
pub(crate) fn image_draw_list(control: &Control) -> DrawList {
    match control {
        Control::Image { image } => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "image sizes are far below f32's exact range"
            )]
            let rect = RectF::new(0.0, 0.0, image.width() as f32, image.height() as f32);
            DrawList::new().image(image.clone(), rect)
        }
        _ => DrawList::new(),
    }
}

/// Creates the window (and, for a spinner, its up-down companion) for
/// `control` under `parent`. Returns `(window, companion)`, or `None` when
/// creation failed. Images are created by the caller, as canvases.
pub(crate) fn create(control: &Control, parent: HWND) -> Option<(HWND, Option<HWND>)> {
    ensure_classes();
    let (class, text, extra) = class_of(control);
    let hwnd = match (create_window(class, &text, extra, parent), control) {
        (Some(hwnd), _) => hwnd,
        (None, Control::Link { text }) => create_window("STATIC", text, SS_NOTIFY, parent)?,
        (None, _) => return None,
    };
    let companion = if let Control::Spinner { .. } = control {
        let updown = create_window(
            "msctls_updown32",
            "",
            UDS_SETBUDDYINT | UDS_ALIGNRIGHT | UDS_ARROWKEYS | UDS_NOTHOUSANDS,
            parent,
        );
        if let Some(updown) = updown {
            // SAFETY: both are live windows of this thread.
            informational(unsafe { SendMessageW(updown, UDM_SETBUDDY, hwnd as usize, 0) });
        }
        updown
    } else {
        None
    };
    sync(hwnd, companion, control);
    Some((hwnd, companion))
}

fn create_window(class: &str, text: &str, extra: u32, parent: HWND) -> Option<HWND> {
    let class = wide(class);
    let text = wide(text);
    // SAFETY: NUL-terminated class and text; `parent` is a live window of
    // this thread; a null `lpParam` is read by none of these classes.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | extra,
            0,
            0,
            0,
            0,
            parent,
            null_mut(),
            module_instance(),
            null(),
        )
    };
    (!hwnd.is_null()).then_some(hwnd)
}

thread_local! {
    /// The control each window last showed, so an unchanged one is not
    /// touched (and a list is not rebuilt, losing its scroll position).
    static SHOWN: RefCell<HashMap<isize, Control>> = RefCell::new(HashMap::new());
}

/// Forgets what `hwnd` showed — called when it is destroyed.
pub(crate) fn forget(hwnd: HWND) {
    SHOWN.with(|shown| shown.borrow_mut().remove(&(hwnd as isize)));
}

fn send(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: `hwnd` is a live control of this thread; every message sent
    // through here takes plain values, or a pointer valid for the call.
    unsafe { SendMessageW(hwnd, message, wparam, lparam) }
}

#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation, reason = "wParam/lParam packing")]
fn signed(value: i64) -> isize {
    value as isize
}

fn set_items(hwnd: HWND, reset: u32, add: u32, items: &[String]) {
    send(hwnd, reset, 0, 0);
    for item in items {
        let text = wide(item);
        send(hwnd, add, 0, text.as_ptr() as isize);
    }
}

fn system_time(date: CalendarDate) -> SYSTEMTIME {
    SYSTEMTIME {
        wYear: u16::try_from(date.year).unwrap_or(1601),
        wMonth: u16::from(date.month),
        wDay: u16::from(date.day),
        ..SYSTEMTIME::default()
    }
}

/// Makes the window show `control`. Idempotent: an unchanged control is
/// not touched.
pub(crate) fn sync(hwnd: HWND, companion: Option<HWND>, control: &Control) {
    let previous = SHOWN.with(|shown| shown.borrow_mut().insert(hwnd as isize, control.clone()));
    if previous.as_ref() == Some(control) {
        return;
    }
    let caption = |text: &str| {
        if window_text(hwnd) != text {
            let text = wide(text);
            // SAFETY: a live window; NUL-terminated text alive for the call.
            unsafe { SetWindowTextW(hwnd, text.as_ptr()) };
        }
    };
    match control {
        Control::Checkbox { label, checked: on } | Control::Toggle { label, on } => {
            caption(label);
            send(hwnd, BM_SETCHECK, usize::from(*on), 0);
        }
        Control::Radio { label, selected } => {
            caption(label);
            send(hwnd, BM_SETCHECK, usize::from(*selected), 0);
        }
        Control::Slider { value, min, max } => {
            send(hwnd, TBM_SETRANGEMIN, 0, signed(*min));
            send(hwnd, TBM_SETRANGEMAX, 0, signed(*max));
            send(hwnd, TBM_SETPOS, 1, signed(*value));
        }
        Control::Progress { percent: Some(percent) } => {
            send(hwnd, PBM_SETPOS, usize::from(*percent), 0);
        }
        Control::Progress { percent: None } => {
            send(hwnd, PBM_SETMARQUEE, 1, 30);
        }
        Control::Select { options, selected } => {
            if !matches!(&previous, Some(Control::Select { options: before, .. }) if before == options)
            {
                set_items(hwnd, CB_RESETCONTENT, CB_ADDSTRING, options);
            }
            send(hwnd, CB_SETCURSEL, selected.unwrap_or(usize::MAX), 0);
        }
        Control::ListBox { items, selected } => {
            if !matches!(&previous, Some(Control::ListBox { items: before, .. }) if before == items)
            {
                set_items(hwnd, LB_RESETCONTENT, LB_ADDSTRING, items);
            }
            send(hwnd, LB_SETCURSEL, selected.unwrap_or(usize::MAX), 0);
        }
        Control::DatePicker { date } => {
            let time = system_time(*date);
            send(hwnd, DTM_SETSYSTEMTIME, GDT_VALID as usize, (&raw const time) as isize);
        }
        Control::Spinner { value, min, max } => {
            if let Some(updown) = companion {
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "wParam carries the signed minimum's bits"
                )]
                let low = signed(*min) as usize;
                send(updown, UDM_SETRANGE32, low, signed(*max));
            }
            caption(&value.to_string());
        }
        Control::Link { text } => {
            if window_text(hwnd).contains("<a>") || window_text(hwnd).is_empty() {
                caption(&format!("<a>{}</a>", escape_link(text)));
            } else {
                caption(text);
            }
        }
        Control::MultilineText { value } => caption(value),
        _ => {}
    }
}

/// Re-attaches a spinner's up-down to its field after the field moved, so
/// the arrows follow it.
pub(crate) fn realign(hwnd: HWND, companion: HWND) {
    send(companion, UDM_SETBUDDY, hwnd as usize, 0);
}

/// Whether a check box or radio button shows checked.
#[must_use]
pub(crate) fn is_checked(hwnd: HWND) -> bool {
    send(hwnd, BM_GETCHECK, 0, 0) == 1
}

/// A trackbar's position.
#[must_use]
pub(crate) fn slider_position(hwnd: HWND) -> i64 {
    i64::try_from(send(hwnd, TBM_GETPOS, 0, 0)).unwrap_or(0)
}

/// A combo or list box's selection.
#[must_use]
pub(crate) fn selection(hwnd: HWND, list: bool) -> Option<usize> {
    usize::try_from(send(hwnd, if list { LB_GETCURSEL } else { CB_GETCURSEL }, 0, 0)).ok()
}

/// A date picker's date.
#[must_use]
pub(crate) fn date(hwnd: HWND) -> Option<CalendarDate> {
    let mut time = SYSTEMTIME::default();
    let valid = send(hwnd, DTM_GETSYSTEMTIME, 0, (&raw mut time) as isize) == 0;
    if !valid {
        return None;
    }
    CalendarDate::new(
        i32::from(time.wYear),
        u8::try_from(time.wMonth).ok()?,
        u8::try_from(time.wDay).ok()?,
    )
}
