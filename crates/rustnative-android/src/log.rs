//! The platform log (`logcat`), under the tag `RustNative`.

#[cfg(target_os = "android")]
mod platform {
    use std::ffi::{CString, c_char, c_int};

    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    pub(super) fn write(priority: c_int, message: &str) {
        let text = CString::new(message.replace('\0', " ")).unwrap_or_default();
        // SAFETY: both pointers are valid NUL-terminated strings for the
        // call; the log copies them.
        unsafe {
            __android_log_write(priority, c"RustNative".as_ptr(), text.as_ptr());
        }
    }
}

/// Logs `message` at info priority.
#[allow(dead_code, reason = "used by the Android build")]
pub(crate) fn info(message: &str) {
    #[cfg(target_os = "android")]
    platform::write(4, message);
    #[cfg(not(target_os = "android"))]
    let _ = message;
}

/// Logs `message` at warning priority.
#[allow(dead_code, reason = "used by the Android build")]
pub(crate) fn warn(message: &str) {
    #[cfg(target_os = "android")]
    platform::write(5, message);
    #[cfg(not(target_os = "android"))]
    let _ = message;
}

/// Logs `message` at error priority.
#[allow(dead_code, reason = "used by the Android build")]
pub(crate) fn error(message: &str) {
    #[cfg(target_os = "android")]
    platform::write(6, message);
    #[cfg(not(target_os = "android"))]
    let _ = message;
}
