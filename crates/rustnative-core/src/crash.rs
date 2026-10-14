//! Crash capture (`PLAN.md` Milestone 51), portable: a panic produces a JSON
//! report — the message, the backtrace, the application and operating-system
//! versions, and the UI tree as it stood — in the application's crash
//! directory. `rustnative crash list` and `rustnative crash show <id>` read
//! them.
//!
//! Every backend shares this; a backend adds only what its host has beyond a
//! panic — the Windows backend writes a minidump beside each report
//! ([`set_dump_writer`]) and reports unhandled exceptions through
//! [`write_report`]. The tree at the moment of failure is the last one this
//! thread viewed ([`Application::view_for`](crate::Application::view_for)
//! records it while capture is on).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// A crash report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    /// Its id (also the dump's file name).
    pub id: String,
    /// When, seconds since the Unix epoch.
    pub time: u64,
    /// What happened: the panic message, or the host's exception code.
    pub message: String,
    /// The backtrace, symbolicated where symbols were available.
    pub backtrace: String,
    /// The application's version.
    pub app_version: String,
    /// The operating system and architecture.
    pub os: String,
    /// The UI tree when it failed (wire JSON), when known.
    pub tree: Option<String>,
    /// The host's dump beside the report, when one was written.
    pub dump: Option<String>,
}

struct Settings {
    directory: PathBuf,
    version: String,
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();
static DUMP_WRITER: OnceLock<fn(&Path) -> bool> = OnceLock::new();

thread_local! {
    static LAST_TREE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The folder reports go to for application `id` on a desktop host: under
/// `%LOCALAPPDATA%` where it is set (Windows), else `$XDG_STATE_HOME` or
/// `~/.local/state` (Linux and other Unix desktops), else the temporary
/// directory. A host without these (Android) passes its own directory to
/// [`install_in`].
#[must_use]
pub fn directory(id: &str) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_STATE_HOME"))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })
        .unwrap_or_else(std::env::temp_dir);
    directory_in(&base, id)
}

/// The folder reports go to for application `id` under `base`.
#[must_use]
pub fn directory_in(base: &Path, id: &str) -> PathBuf {
    base.join(id).join("crashes")
}

/// Turns crash capture on for application `id` at `version`, reporting to
/// [`directory`]`(id)`.
pub fn install(id: &str, version: &str) {
    install_in(directory(id), version);
}

/// Turns crash capture on, reporting to `directory`. The first call wins;
/// the panic hook it installs chains to the one before it.
pub fn install_in(directory: PathBuf, version: &str) {
    if SETTINGS.set(Settings { directory, version: version.to_owned() }).is_err() {
        return;
    }
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
        let dump = DUMP_WRITER.get().copied();
        let _ = write_report(&format!("panicked: {message}{location}"), dump);
        previous(info);
    }));
}

/// Whether crash capture is on.
#[must_use]
pub fn installed() -> bool {
    SETTINGS.get().is_some()
}

/// Gives every report a host dump: `writer` writes one at the path it is
/// handed and says whether it did. The first call wins.
pub fn set_dump_writer(writer: fn(&Path) -> bool) {
    let _ = DUMP_WRITER.set(writer);
}

/// Keeps `tree` (wire JSON) as the tree to report if this thread crashes,
/// computing it only while capture is on.
pub fn record_tree(tree: impl FnOnce() -> String) {
    if installed() {
        LAST_TREE.with(|last| *last.borrow_mut() = Some(tree()));
    }
}

/// Writes a report of `message` now, with a dump from `dump` when given,
/// returning the report's path — for a host's own failure path (an
/// unhandled exception) as well as the panic hook. `None` when capture is
/// off or the report cannot be written.
pub fn write_report(message: &str, dump: Option<impl FnOnce(&Path) -> bool>) -> Option<PathBuf> {
    let settings = SETTINGS.get()?;
    std::fs::create_dir_all(&settings.directory).ok()?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let id = format!("{time}-{}", std::process::id());
    let dump = dump.and_then(|write| {
        let path = settings.directory.join(format!("{id}.dmp"));
        write(&path).then(|| path.display().to_string())
    });
    let report = CrashReport {
        id: id.clone(),
        time,
        message: message.to_owned(),
        backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        app_version: settings.version.clone(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        tree: LAST_TREE.with(|last| last.try_borrow().ok().and_then(|tree| tree.clone())),
        dump,
    };
    let path = settings.directory.join(format!("{id}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&report).ok()?).ok()?;
    Some(path)
}

/// The reports in `directory`, newest first.
#[must_use]
pub fn reports(directory: &Path) -> Vec<CrashReport> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_directory_is_per_application() {
        let base = Path::new("base");
        assert_eq!(directory_in(base, "dev.example"), base.join("dev.example").join("crashes"));
        assert!(directory("dev.example").ends_with(Path::new("dev.example").join("crashes")));
    }

    #[test]
    fn reports_are_read_newest_first_and_others_skipped() {
        let folder =
            std::env::temp_dir().join(format!("rustnative-crash-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for time in [10, 30, 20] {
            let report = CrashReport {
                id: time.to_string(),
                time,
                message: "m".into(),
                backtrace: String::new(),
                app_version: "1".into(),
                os: "test".into(),
                tree: None,
                dump: None,
            };
            std::fs::write(
                folder.join(format!("{time}.json")),
                serde_json::to_string(&report).unwrap(),
            )
            .unwrap();
        }
        std::fs::write(folder.join("note.txt"), "not a report").unwrap();
        let times: Vec<u64> = reports(&folder).iter().map(|report| report.time).collect();
        assert_eq!(times, [30, 20, 10]);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
