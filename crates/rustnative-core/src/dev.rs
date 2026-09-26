//! What a development run adds (`PLAN.md` Milestone 43): `rustnative dev`
//! starts the application with `RUSTNATIVE_DEV=1`, and the application
//! then reports panics with their source position — the `.rsx` position
//! when the panicking code was lowered from markup — and finds the
//! development resources its `rustnative.toml` declares.
//!
//! ```
//! // Outside `rustnative dev` there is nothing to find.
//! assert!(rustnative_core::dev::resource("uploads").is_none() || rustnative_core::dev::is_dev());
//! ```

use std::path::PathBuf;
use std::sync::Mutex;

/// The environment variable `rustnative dev` sets.
pub const DEV_VARIABLE: &str = "RUSTNATIVE_DEV";

/// Whether this is a development run.
#[must_use]
pub fn is_dev() -> bool {
    std::env::var(DEV_VARIABLE).is_ok_and(|value| value == "1")
}

/// The folder or file `rustnative dev` provisioned for the resource `name`
/// declared in `rustnative.toml`'s `[resources]` (passed as
/// `RUSTNATIVE_RESOURCE_<NAME>`), if it did.
#[must_use]
pub fn resource(name: &str) -> Option<PathBuf> {
    let variable = format!("RUSTNATIVE_RESOURCE_{}", name.to_ascii_uppercase().replace('-', "_"));
    std::env::var_os(variable).map(PathBuf::from)
}

/// Where a panic happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicSite {
    /// What it said.
    pub message: String,
    /// The Rust file, line, and column.
    pub file: String,
    /// The line.
    pub line: u32,
    /// The column.
    pub column: u32,
    /// The `.rsx` file the Rust file was lowered from, when it was.
    pub markup: Option<String>,
}

impl std::fmt::Display for PanicSite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = self.markup.as_deref().unwrap_or(&self.file);
        write!(f, "{}\n\nat {file}:{}:{}", self.message, self.line, self.column)
    }
}

static LAST_PANIC: Mutex<Option<PanicSite>> = Mutex::new(None);

/// The `.rsx` file `file` was lowered from, read from the source map
/// `rustnative_build::compile_rsx` writes beside it (`<file>.map`). Lowering
/// keeps lines, so the line is the same.
#[must_use]
pub fn markup_source(file: &str) -> Option<String> {
    let map = std::fs::read_to_string(format!("{file}.map")).ok()?;
    let map: serde_json::Value = serde_json::from_str(&map).ok()?;
    map.get("source")?.as_str().map(str::to_owned)
}

/// In a development run, records where each panic happens (and still
/// reports it as before), for [`take_panic`]. Does nothing otherwise.
pub fn capture_panics() {
    if !is_dev() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with a non-string payload".to_owned());
        if let Some(location) = info.location() {
            let site = PanicSite {
                message,
                file: location.file().to_owned(),
                line: location.line(),
                column: location.column(),
                markup: markup_source(location.file()),
            };
            *LAST_PANIC.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(site);
        }
        previous(info);
    }));
}

/// The last panic [`capture_panics`] recorded, if any.
#[must_use]
pub fn take_panic() -> Option<PanicSite> {
    LAST_PANIC.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lowered_file_names_the_markup_it_came_from() {
        let directory = std::env::temp_dir().join(format!("rustnative-dev-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let lowered = directory.join("app.rs");
        std::fs::write(
            format!("{}.map", lowered.display()),
            r#"{"source":"src/app.rsx","lines":{}}"#,
        )
        .unwrap();
        assert_eq!(markup_source(&lowered.display().to_string()).as_deref(), Some("src/app.rsx"));
        assert_eq!(markup_source("no/such/file.rs"), None);
        let site = PanicSite {
            message: "boom".into(),
            file: lowered.display().to_string(),
            line: 12,
            column: 5,
            markup: Some("src/app.rsx".into()),
        };
        assert!(site.to_string().ends_with("at src/app.rsx:12:5"));
        let _ = std::fs::remove_dir_all(directory);
    }
}
