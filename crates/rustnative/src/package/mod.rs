//! `rustnative package`: what a person downloads or installs.
//!
//! Two formats, for the two ways a Windows application is shipped:
//!
//! - a **portable zip** — the executable and its files, byte-for-byte
//!   reproducible with a `SHA256SUMS` beside it, for people who unzip and
//!   run;
//! - an **MSIX** — the packaged, installable, updatable form, with the
//!   application's identity, its logos, and the URL schemes it handles,
//!   signed if a certificate is given.
//!
//! What goes *into* the executable — its icon, version, and manifest — is
//! not here: that is `rustnative-build`, which the application's own
//! `build.rs` runs, so an executable carries its resources however it was
//! built, not only when it was packaged.

pub mod msix;
pub mod sign;
pub mod zip;

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::toolchain::{cargo, windows_sdk::WindowsToolchain};

/// Which packages to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum Format {
    /// A reproducible portable archive with a `SHA256SUMS`.
    Zip,
    /// An installable, optionally signed MSIX package.
    Msix,
    /// Both.
    All,
}

impl Format {
    const fn wants_zip(self) -> bool {
        matches!(self, Self::Zip | Self::All)
    }

    const fn wants_msix(self) -> bool {
        matches!(self, Self::Msix | Self::All)
    }
}

/// Builds the application and packages it, returning what was produced.
///
/// # Errors
///
/// The build's error, a missing SDK tool, or anything that could not be
/// written.
pub fn package(
    root: &Path,
    config: &Config,
    format: Format,
    signing: Option<&sign::Signing>,
) -> Result<Vec<PathBuf>> {
    if signing.is_some() {
        // Checked before the build, so a missing publisher is not found
        // after a long compile.
        sign::check_publisher(config.app.publisher.as_deref())?;
    }
    cargo::run(root, ["build", "--release"])?;

    let executable = executable_path(root, &config.app.name)?;
    let output = root.join("target").join("package");
    std::fs::create_dir_all(&output)
        .map_err(|cause| Error::Io { what: "create target/package".to_owned(), cause })?;

    let mut produced = Vec::new();
    if format.wants_zip() {
        produced.push(build_zip(&output, &executable, config)?);
    }
    if format.wants_msix() {
        produced.push(build_msix(root, &output, &executable, config, signing)?);
    }
    Ok(produced)
}

/// Where `cargo build --release` puts the executable.
fn executable_path(root: &Path, name: &str) -> Result<PathBuf> {
    // `CARGO_TARGET_DIR` moves it, which the framework's own tests rely on.
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let executable = target.join("release").join(format!("{name}.exe"));
    if executable.is_file() {
        Ok(executable)
    } else {
        Err(Error::Io {
            what: format!("find the built executable at {}", executable.display()),
            cause: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        })
    }
}

/// Writes the portable archive and returns its path.
fn build_zip(output: &Path, executable: &Path, config: &Config) -> Result<PathBuf> {
    let name = format!("{}-{}", config.app.name, config.app.version);
    let mut entries = vec![
        zip::entry_from(executable, &format!("{}.exe", config.app.name))
            .map_err(|cause| Error::Io { what: "read the built executable".to_owned(), cause })?,
    ];
    entries.push(zip::Entry {
        name: "SHA256SUMS".to_owned(),
        data: zip::checksums(&entries).into_bytes(),
    });
    let archive = zip::archive(entries);
    let path = output.join(format!("{name}.zip"));
    std::fs::write(&path, archive)
        .map_err(|cause| Error::Io { what: format!("write {}", path.display()), cause })?;
    Ok(path)
}

/// Builds (and optionally signs) the MSIX, returning its path.
fn build_msix(
    root: &Path,
    output: &Path,
    executable: &Path,
    config: &Config,
    signing: Option<&sign::Signing>,
) -> Result<PathBuf> {
    let toolchain = WindowsToolchain::detect();
    let makeappx =
        toolchain.tool("makeappx").and_then(|tool| tool.path.clone()).ok_or_else(|| {
            Error::ToolMissing {
                tool: "makeappx",
                hint: "install the Windows SDK (run `rustnative doctor`)".to_owned(),
                cause: None,
            }
        })?;

    let layout = msix::layout_directory(output, &config.app.name);
    let _ = std::fs::remove_dir_all(&layout);
    let icon = config.app.icon.as_ref().map(|icon| root.join(icon));
    msix::lay_out(&layout, executable, config, icon.as_deref())?;

    let path = output.join(format!("{}-{}.msix", config.app.name, config.app.version));
    msix::pack(&makeappx, &layout, &path)?;

    if let Some(signing) = signing {
        let signtool =
            toolchain.tool("signtool").and_then(|tool| tool.path.clone()).ok_or_else(|| {
                Error::ToolMissing {
                    tool: "signtool",
                    hint: "install the Windows SDK (run `rustnative doctor`)".to_owned(),
                    cause: None,
                }
            })?;
        sign::sign(&signtool, signing, &path)?;
    }
    Ok(path)
}

/// XML-escapes `text` for the manifests this module writes.
pub(crate) fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_say_what_they_produce() {
        assert!(Format::Zip.wants_zip() && !Format::Zip.wants_msix());
        assert!(Format::Msix.wants_msix() && !Format::Msix.wants_zip());
        assert!(Format::All.wants_zip() && Format::All.wants_msix());
    }

    #[test]
    fn xml_escaping_covers_everything_an_attribute_cannot_hold() {
        assert_eq!(escape_xml(r#"a&b<c>d"e'f"#), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
    }
}
