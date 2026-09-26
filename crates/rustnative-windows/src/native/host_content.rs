//! Host content controls on Windows (`PLAN.md` Milestone 48, `C28`):
//! [`rustnative_core::HostContent`] realized with the host's own controls.
//!
//! - **Media** is an `MCIWnd` window (`msvfw32`): the system's media
//!   control, with its own play bar, seek, and volume, opening a file
//!   path through MCI.
//! - **Camera** is an `avicap32` capture window connected to the camera
//!   driver and previewing it. The capability is present only when a
//!   capture driver is installed.
//! - **Web** content needs `WebView2`, whose loader is not part of Windows'
//!   own API surface; this backend answers it `Unavailable` (no
//!   `Capability::WebContent`), so `host_content` builds the
//!   application's fallback. Recorded in `BUILD_STATUS.md`.
//!
//! Owed: the system media transport controls (SMTC) and picture-in-picture
//! for media, both `WinRT` APIs outside `windows-sys`.

use rustnative_core::{HostContent, Size};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Media::Multimedia::{
    MCIWNDF_NOERRORDLG, MCIWNDF_NOMENU, MCIWndCreateW, WM_CAP_DRIVER_CONNECT, WM_CAP_SET_PREVIEW,
    WM_CAP_SET_PREVIEWRATE, WM_CAP_SET_SCALE, capCreateCaptureWindowW, capGetDriverDescriptionW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WS_CHILD, WS_VISIBLE};

use super::foreign::{ForeignControl, Ownership};
use super::util::wide;

/// The natural size of host content, before layout fixes one.
pub(crate) fn preferred_size(content: &HostContent) -> Size {
    match content {
        HostContent::Web { .. } => Size::new(640, 480),
        HostContent::Media { .. } => Size::new(480, 320),
        HostContent::Camera { .. } => Size::new(320, 240),
    }
}

/// Whether a camera capture driver is installed.
pub(crate) fn camera_available() -> bool {
    let mut name = [0u16; 80];
    let mut version = [0u16; 80];
    // SAFETY: both buffers are valid for the lengths given (in `u16`s,
    // which is what the counts mean for the wide variant).
    unsafe { capGetDriverDescriptionW(0, name.as_mut_ptr(), 80, version.as_mut_ptr(), 80) != 0 }
}

/// Creates the host control for `content` inside `parent`, or `None` where
/// this backend has none (web content) or the host refused.
pub(crate) fn create(content: &HostContent, parent: HWND) -> Option<ForeignControl> {
    let hwnd = match content {
        HostContent::Media { source } => {
            let source = wide(source);
            // SAFETY: `parent` is a live window; `source` is NUL-terminated
            // and outlives the call. A null instance means this module's.
            unsafe {
                MCIWndCreateW(
                    parent,
                    std::ptr::null_mut(),
                    WS_CHILD | WS_VISIBLE | MCIWNDF_NOMENU | MCIWNDF_NOERRORDLG,
                    source.as_ptr(),
                )
            }
        }
        HostContent::Camera { device } => {
            let name = wide("camera");
            // SAFETY: `parent` is a live window and `name` is NUL-terminated.
            let hwnd = unsafe {
                capCreateCaptureWindowW(
                    name.as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    0,
                    0,
                    320,
                    240,
                    parent,
                    0,
                )
            };
            if !hwnd.is_null() {
                // SAFETY: `hwnd` is the capture window just created; these
                // messages take plain integers. A driver that will not
                // connect leaves a blank preview, which is what the host
                // shows too.
                unsafe {
                    SendMessageW(hwnd, WM_CAP_DRIVER_CONNECT, *device as usize, 0);
                    SendMessageW(hwnd, WM_CAP_SET_SCALE, 1, 0);
                    SendMessageW(hwnd, WM_CAP_SET_PREVIEWRATE, 66, 0);
                    SendMessageW(hwnd, WM_CAP_SET_PREVIEW, 1, 0);
                }
            }
            hwnd
        }
        HostContent::Web { .. } => return None,
    };
    (!hwnd.is_null()).then_some(ForeignControl { hwnd, ownership: Ownership::Owned })
}
