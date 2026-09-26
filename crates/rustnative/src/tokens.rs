//! `rustnative tokens import <file>` (`PLAN.md` Milestone 48): a design
//! system's W3C Design Tokens file into the project's style file, as its
//! `@theme` block (`docs/tokens.md`).

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::project::Project;

/// Imports `file` into the project's style file (or `out`), replacing the
/// block a previous import wrote.
///
/// # Errors
///
/// The project, the token file, or the style file cannot be read or
/// written, or the token file is not a token file.
pub fn import(here: &Path, file: &Path, out: Option<PathBuf>) -> Result<()> {
    let project = Project::find(here)?;
    let json = std::fs::read_to_string(file)
        .map_err(|cause| Error::Io { what: format!("read {}", file.display()), cause })?;
    let imported = rustnative_style::design_tokens::import(&json).map_err(Error::Usage)?;
    let style = out
        .or_else(|| rustnative_build::styles::style_file(&project.root))
        .unwrap_or_else(|| project.root.join("app.css"));
    let existing = std::fs::read_to_string(&style).unwrap_or_default();
    let updated = rustnative_style::design_tokens::replace_block(&existing, &imported.css);
    // The result must compile, exactly as the build will compile it.
    rustnative_build::styles::theme_source(&style, &updated).map_err(Error::Usage)?;
    std::fs::write(&style, updated)
        .map_err(|cause| Error::Io { what: format!("write {}", style.display()), cause })?;
    let count = imported.css.lines().filter(|line| line.trim_start().starts_with("--")).count();
    println!("tokens: {count} tokens into {}", style.display());
    for (token, role) in &imported.host_roles {
        println!("tokens: --{token} follows the host's {role}");
    }
    for warning in &imported.warnings {
        eprintln!("tokens: {warning}");
    }
    Ok(())
}
