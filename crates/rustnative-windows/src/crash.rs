//! Crash capture (`PLAN.md` Milestone 51): a panic or an unhandled
//! exception produces a minidump and a JSON report — the backtrace
//! (symbolicated in process when the PDB is beside the executable), the
//! OS and application versions, and the UI tree as it stood — in
//! `%LOCALAPPDATA%\<app id>\crashes\`. `rustnative crash list` and
//! `rustnative crash show <id>` read them.
//!
//! The tree at the moment of failure is the last one this thread rendered,
//! kept as wire data while crash capture is on ([`record_tree`], called by
//! the runtime after each render).

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL};
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MiniDumpWithDataSegs, MiniDumpWriteDump,
    SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
};

/// A crash report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    /// Its id (also the dump's file name).
    pub id: String,
    /// When, seconds since the Unix epoch.
    pub time: u64,
    /// What happened: the panic message, or the exception code.
    pub message: String,
    /// The backtrace, symbolicated where symbols were available.
    pub backtrace: String,
    /// The application's version.
    pub app_version: String,
    /// The operating system.
    pub os: String,
    /// The UI tree when it failed (wire JSON), when known.
    pub tree: Option<String>,
    /// The minidump beside the report, when one was written.
    pub dump: Option<String>,
}

struct Settings {
    directory: PathBuf,
    version: String,
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

thread_local! {
    static LAST_TREE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The folder reports go to for application `id`:
/// `%LOCALAPPDATA%\<id>\crashes`.
#[must_use]
pub fn directory(id: &str) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
    directory_in(&base, id)
}

/// The folder reports go to for application `id` under `local_app_data`.
#[must_use]
pub fn directory_in(local_app_data: &std::path::Path, id: &str) -> PathBuf {
    local_app_data.join(id).join("crashes")
}

/// Turns crash capture on for application `id` at `version`.
pub fn install(id: &str, version: &str) {
    let _ = SETTINGS.set(Settings { directory: directory(id), version: version.to_owned() });
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".to_owned());
        let location = info
            .location()
            .map(|location| format!(" at {}:{}", location.file(), location.line()))
            .unwrap_or_default();
        let _ = write_report(&format!("panicked: {message}{location}"), None);
        previous(info);
    }));
    // SAFETY: the filter is a plain `extern "system"` function that lives
    // for the whole process.
    unsafe {
        SetUnhandledExceptionFilter(Some(on_exception));
    }
}

/// Whether crash capture is on (the runtime records trees only then).
#[must_use]
pub fn installed() -> bool {
    SETTINGS.get().is_some()
}

/// Keeps `tree` (wire JSON) as the tree to report if this thread crashes.
pub fn record_tree(tree: impl FnOnce() -> String) {
    if installed() {
        LAST_TREE.with(|last| *last.borrow_mut() = Some(tree()));
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
    let _ = write_report(&format!("unhandled exception 0x{code:08X}"), Some(pointers));
    0 // EXCEPTION_CONTINUE_SEARCH: let Windows end the process as usual.
}

fn write_dump(path: &std::path::Path, pointers: Option<*const EXCEPTION_POINTERS>) -> bool {
    let wide: Vec<u16> = path.as_os_str().encode_wide_null();
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

trait EncodeWideNull {
    fn encode_wide_null(&self) -> Vec<u16>;
}

impl EncodeWideNull for std::ffi::OsStr {
    fn encode_wide_null(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

fn write_report(message: &str, pointers: Option<*const EXCEPTION_POINTERS>) -> Option<PathBuf> {
    let settings = SETTINGS.get()?;
    std::fs::create_dir_all(&settings.directory).ok()?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let id = format!("{time}-{}", std::process::id());
    let dump_path = settings.directory.join(format!("{id}.dmp"));
    let dump = write_dump(&dump_path, pointers).then(|| dump_path.display().to_string());
    let report = CrashReport {
        id: id.clone(),
        time,
        message: message.to_owned(),
        backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        app_version: settings.version.clone(),
        os: format!("Windows {}", std::env::consts::ARCH),
        tree: LAST_TREE.with(|last| last.try_borrow().ok().and_then(|tree| tree.clone())),
        dump,
    };
    let path = settings.directory.join(format!("{id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&report).ok()?).ok()?;
    Some(path)
}

/// The reports in `directory`, newest first.
#[must_use]
pub fn reports(directory: &std::path::Path) -> Vec<CrashReport> {
    let mut reports: Vec<CrashReport> = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.path().extension().is_some_and(|extension| extension == "json")
                })
                .filter_map(|entry| {
                    serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()
                })
                .collect()
        })
        .unwrap_or_default();
    reports.sort_by_key(|report| std::cmp::Reverse(report.time));
    reports
}
