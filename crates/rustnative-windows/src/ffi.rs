//! Small FFI helpers shared across the Windows backend's services and
//! native window code.

/// Encodes `value` as a NUL-terminated UTF-16 buffer, the shape almost every
/// Win32 string-in string-out API expects.
#[cfg(windows)]
pub(crate) fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

// `windows-sys` does not expose `GlobalFree` under the `Win32_System_Memory`
// feature at the versions this workspace pins, so it is declared directly
// here rather than pulling in an extra feature just for one function.
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn GlobalFree(
        memory: windows_sys::Win32::Foundation::HGLOBAL,
    ) -> windows_sys::Win32::Foundation::HGLOBAL;
}
