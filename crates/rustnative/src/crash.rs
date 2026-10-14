//! `rustnative crash` (`PLAN.md` Milestone 51): the crash reports an
//! application left on this machine, in `rustnative_core::crash`'s format
//! and directory (`%LOCALAPPDATA%\<id>\crashes` on Windows,
//! `$XDG_STATE_HOME/<id>/crashes` on Linux). An Android application keeps
//! its reports in its own files directory on the device.

use std::path::Path;

use rustnative_core::crash::{directory, reports};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::project::Project;

/// `rustnative crash`.
#[derive(Debug, clap::Subcommand)]
pub enum CrashCommand {
    /// List the reports, newest first.
    List,
    /// Show one report: the message, the backtrace, and the tree at failure.
    Show {
        /// The report's id (from `list`).
        id: String,
    },
}

/// Runs a `crash` command.
///
/// # Errors
///
/// The project cannot be read, or there is no such report.
pub fn run(here: &Path, command: &CrashCommand) -> Result<()> {
    let project = Project::find(here)?;
    let directory = directory(&project.config.app.id);
    let reports = reports(&directory);
    match command {
        CrashCommand::List => {
            if reports.is_empty() {
                println!("crash: no reports in {}", directory.display());
            }
            for report in &reports {
                println!("{}  {}  {}", report.id, report.app_version, report.message);
            }
            Ok(())
        }
        CrashCommand::Show { id } => {
            let report = reports.iter().find(|report| report.id == *id).ok_or_else(|| {
                Error::Usage(format!("no crash report {id} in {}", directory.display()))
            })?;
            println!("{}", report.message);
            println!("version {}  {}", report.app_version, report.os);
            if let Some(dump) = &report.dump {
                println!("dump {dump}");
            }
            println!("\n{}", report.backtrace);
            if let Some(tree) = report.tree.as_deref() {
                let pretty = serde_json::from_str::<Value>(tree)
                    .ok()
                    .and_then(|tree| serde_json::to_string_pretty(&tree).ok())
                    .unwrap_or_else(|| tree.to_owned());
                println!("\nThe tree when it failed:\n{pretty}");
            }
            Ok(())
        }
    }
}
