//! Crash capture on Android (`PLAN.md` Milestone 51): the portable report of
//! `rustnative_core::crash`, kept in the package's own files directory
//! (`getFilesDir()/crashes`), where nothing else can read it.

pub use rustnative_core::crash::{CrashReport, installed, record_tree, reports};

/// Turns crash capture on for this application at `version`.
///
/// # Errors
///
/// There is no files directory (the activity has not started).
#[cfg(target_os = "android")]
pub fn install(version: &str) -> Result<(), rustnative_core::ServiceError> {
    rustnative_core::crash::install_in(crate::services::files_dir()?.join("crashes"), version);
    Ok(())
}
