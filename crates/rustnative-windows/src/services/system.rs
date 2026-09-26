//! Native shell integration: launching URLs and posting tray notifications.

#[cfg(windows)]
use super::{notifications, run_blocking};
#[cfg(windows)]
use crate::ffi::wide_string;

/// Native shell integration. `open_url` delegates to the user's registered
/// browser and returns an explicit error when Windows rejects the request.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsSystem;

#[cfg(windows)]
#[async_trait::async_trait]
impl rustnative_core::SystemService for WindowsSystem {
    async fn open_url(&self, url: String) -> Result<(), rustnative_core::ServiceError> {
        run_blocking(move || {
            use windows_sys::Win32::UI::Shell::ShellExecuteW;
            use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            let file = wide_string(&url);
            // SAFETY: all pointer arguments are either null (hwnd, verb,
            // directory) or point at `file`, a NUL-terminated UTF-16 buffer
            // we own for the duration of this synchronous call.
            let result = unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    file.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
            if (result as isize) <= 32 {
                return Err(rustnative_core::ServiceError::new("ShellExecuteW failed to open URL"));
            }
            Ok(())
        })
        .await
    }

    async fn notify(
        &self,
        title: String,
        body: String,
    ) -> Result<(), rustnative_core::ServiceError> {
        run_blocking(move || {
            use windows_sys::Win32::UI::Shell::{
                NIF_ICON, NIF_INFO, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_MODIFY, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            };
            use windows_sys::Win32::UI::WindowsAndMessaging::{IDI_INFORMATION, LoadIconW};

            let mut host = notifications::notification_host()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let host = host.as_mut().map_err(|message| {
                rustnative_core::ServiceError::new(format!(
                    "failed to create the notification host window: {message}"
                ))
            })?;

            // SAFETY: a null hwnd and a standard system icon ID are a
            // documented-valid `LoadIconW` call; the returned handle is a
            // static system resource that does not need `DestroyIcon`.
            let icon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_INFORMATION) };

            // SAFETY: `zeroed()` is a valid initial bit pattern for
            // `NOTIFYICONDATAW`, a plain-old-data Win32 struct with no
            // internal invariants beyond `cbSize` being set before use,
            // which happens on the next line.
            let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
            // This struct's size can never approach `u32::MAX`.
            #[allow(clippy::cast_possible_truncation)]
            let struct_size = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.cbSize = struct_size;
            // A real, stable `HWND` (rather than the documented-but-
            // undefined-behavior-adjacent `NULL`) is what gives this
            // notification icon a well-defined identity across repeated
            // calls — see `NotificationHost`'s doc comment (standards
            // audit P1.4).
            data.hWnd = host.hwnd;
            data.uID = 1;
            data.uFlags = NIF_ICON | NIF_TIP | NIF_INFO;
            data.hIcon = icon;
            data.dwInfoFlags = NIIF_INFO;
            notifications::copy_into_wide_buffer(&title, &mut data.szInfoTitle);
            notifications::copy_into_wide_buffer(&body, &mut data.szInfo);
            notifications::copy_into_wide_buffer(&title, &mut data.szTip);

            // The icon is added exactly once and updated in place for
            // every subsequent notification (`NIM_MODIFY`) rather than
            // torn down and recreated per call: besides matching the
            // audit's "maintain the icon until the notification lifecycle
            // is complete" guidance, an add/delete cycle on every call is
            // what caused the previous design's taskbar icon to visibly
            // flicker in and out for each notification.
            let message = if host.icon_added { NIM_MODIFY } else { NIM_ADD };
            // SAFETY: `data` is a fully initialized, correctly sized
            // `NOTIFYICONDATAW` owned on this stack frame for the duration
            // of this call; `data.hWnd` is `host.hwnd`, a window created by
            // `create_notification_host` and never destroyed for the
            // remainder of the process, so it remains a valid handle for
            // every call this function makes over the process's lifetime.
            if unsafe { Shell_NotifyIconW(message, &raw const data) } == 0 {
                return Err(rustnative_core::ServiceError::new(format!(
                    "Shell_NotifyIconW({}) failed to post the notification",
                    if host.icon_added { "NIM_MODIFY" } else { "NIM_ADD" }
                )));
            }
            host.icon_added = true;
            Ok(())
        })
        .await
    }
}
