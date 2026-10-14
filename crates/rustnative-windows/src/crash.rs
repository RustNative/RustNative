//! Crash capture on Windows (`PLAN.md` Milestone 51). The report — the
//! message, the backtrace (symbolicated in process when the PDB is beside
//! the executable), the versions, and the UI tree as it stood — is
//! `rustnative_core::crash`'s, shared with every backend, in
//! `%LOCALAPPDATA%\<app id>\crashes\`. What Windows adds: a minidump beside
//! each report, and a report for an unhandled structured exception as well
//! as a panic. `rustnative crash list` and `rustnative crash show <id>` read
//! them.

pub use rustnative_core::crash::{
    CrashReport, directory, directory_in, installed, record_tree, reports,
};
use windows_sys::Win32::Foundation::{GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL};
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MiniDumpWithDataSegs, MiniDumpWriteDump,
    SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
};

/// Turns crash capture on for application `id` at `version`: the portable
/// panic report, a minidump beside each, and unhandled exceptions reported.
pub fn install(id: &str, version: &str) {
    rustnative_core::crash::install(id, version);
    rustnative_core::crash::set_dump_writer(|path| write_dump(path, None));
    // SAFETY: the filter is a plain `extern "system"` function that lives
    // for the whole process.
    unsafe {
        SetUnhandledExceptionFilter(Some(on_exception));
    }
}

unsafe extern "system" fn on_exception(pointers: *const EXCEPTION_POINTERS) -> i32 {
    // SAFETY: Windows passes a valid EXCEPTION_POINTERS to the filter.
    let code = unsafe {
        pointers
            .as_ref()
            .and_then(|pointers| pointers.ExceptionRecord.as_ref())
            .map_or(0, |record| record.ExceptionCode)
    };
    let _ = rustnative_core::crash::write_report(
        &format!("unhandled exception 0x{code:08X}"),
        Some(|path: &std::path::Path| write_dump(path, Some(pointers))),
    );
    0 // EXCEPTION_CONTINUE_SEARCH: let Windows end the process as usual.
}

fn write_dump(path: &std::path::Path, pointers: Option<*const EXCEPTION_POINTERS>) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated path alive for the call.
    let file: HANDLE = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return false;
    }
    let information = pointers.map(|pointers| MINIDUMP_EXCEPTION_INFORMATION {
        // SAFETY: plain thread id query.
        ThreadId: unsafe { GetCurrentThreadId() },
        ExceptionPointers: pointers.cast_mut(),
        ClientPointers: 0,
    });
    // SAFETY: the process and file handles are valid; the exception
    // information, when present, points at a live structure for the call.
    let written = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file,
            MiniDumpWithDataSegs,
            information.as_ref().map_or(std::ptr::null(), std::ptr::from_ref),
            std::ptr::null(),
            std::ptr::null(),
        )
    } != 0;
    // SAFETY: closing the handle this function opened.
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(file);
    }
    written
}
