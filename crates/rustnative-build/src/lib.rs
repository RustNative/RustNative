//! Build-script support for Rust Native applications: the icon, manifest,
//! and version information a Windows executable carries.
//!
//! An application's `build.rs` is one line:
//!
//! ```text
//! fn main() {
//!     rustnative_build::embed_resources();
//! }
//! ```
//!
//! which reads the project's `rustnative.toml`, writes an `.ico`, an application
//! manifest, and a `.rc` into the build directory, compiles them with the
//! Windows SDK's `rc.exe`, and tells Cargo to link the result. The
//! executable then has an icon in Explorer, a version in its properties,
//! and — the part this framework depends on rather than merely likes —
//! per-monitor DPI awareness, Common Controls v6, and the `supportedOS`
//! entries without which layered child windows (Milestone 27's opacity)
//! and themed tab controls (Milestone 30) do not behave as documented.
//!
//! # When the SDK is not there
//!
//! Resource compilation is skipped with a Cargo warning rather than
//! failing the build: an application still runs without an icon, and a
//! contributor without the SDK can still `cargo check`. `rustnative doctor` is
//! where a missing SDK is reported as a problem.

#![deny(missing_docs)]

pub mod android;
pub mod assets;
pub mod icon;
pub mod manifest;
pub mod markup;
pub mod messages;
pub mod rc;
pub mod sdk;
pub mod styles;

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use assets::embed_assets;
pub use markup::{compile_rsx, try_compile_rsx};
pub use messages::compile_messages;
pub use styles::compile_styles;

/// What `rustnative.toml` says, as much of it as resources need.
#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    app: App,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct App {
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    version: String,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<PathBuf>,
}

/// Why resources could not be embedded.
#[derive(Debug)]
pub enum BuildError {
    /// `rustnative.toml` is missing, unreadable, or not valid.
    Manifest(String),
    /// A file could not be written into the build directory.
    Io(std::io::Error),
    /// The icon could not be prepared.
    Icon(icon::IconError),
    /// `rc.exe` ran and failed.
    ResourceCompiler(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(message) => write!(f, "rustnative.toml: {message}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Icon(error) => write!(f, "{error}"),
            Self::ResourceCompiler(message) => write!(f, "rc.exe: {message}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Embeds the application's resources, printing a Cargo warning and
/// carrying on if the Windows SDK is not installed.
///
/// # Panics
///
/// Panics only if `rustnative.toml` itself is wrong — a mistake in the project
/// rather than in the machine, and one that should stop the build.
pub fn embed_resources() {
    if !cfg!(windows) {
        return;
    }
    match try_embed_resources() {
        Ok(true) => {}
        Ok(false) => {
            println!(
                "cargo:warning=the Windows SDK's rc.exe was not found: this build has no icon, \
                 version information, or application manifest (run `rustnative doctor`)"
            );
        }
        Err(error @ BuildError::Manifest(_)) => panic!("{error}"),
        Err(error) => println!("cargo:warning=resources were not embedded: {error}"),
    }
}

/// Embeds the resources, reporting whether `rc.exe` was found and run.
///
/// # Errors
///
/// [`BuildError`] for a bad `rustnative.toml`, a file that could not be written, an
/// unusable icon, or a failing `rc.exe`.
pub fn try_embed_resources() -> Result<bool, BuildError> {
    let project = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| BuildError::Manifest("CARGO_MANIFEST_DIR is not set".to_owned()))?;
    let out = std::env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| BuildError::Manifest("OUT_DIR is not set".to_owned()))?;
    let resources = read_manifest(&project)?;
    println!("cargo:rerun-if-changed=rustnative.toml");
    if let Some(icon) = &resources.icon_file {
        println!("cargo:rerun-if-changed={icon}");
    }
    prepare(&project, &out, &resources)?;

    let Some(compiler) = sdk::resource_compiler() else {
        return Ok(false);
    };
    let script = out.join(format!("{}.rc", resources.name));
    let object = out.join(format!("{}.res", resources.name));
    let output = std::process::Command::new(compiler)
        .current_dir(&out)
        .arg("/nologo")
        .arg("/fo")
        .arg(&object)
        .arg(&script)
        .output()
        .map_err(BuildError::Io)?;
    if !output.status.success() {
        return Err(BuildError::ResourceCompiler(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        ));
    }
    println!("cargo:rustc-link-arg-bins={}", object.display());
    Ok(true)
}

/// Writes the icon, manifest, and script into `out`, ready to compile.
///
/// # Errors
///
/// [`BuildError::Io`] or [`BuildError::Icon`].
fn prepare(project: &Path, out: &Path, resources: &rc::Resources) -> Result<(), BuildError> {
    if let Some(icon) = &resources.icon_file {
        let source = project.join(resources.icon_source.as_ref().unwrap_or(&PathBuf::from(icon)));
        let ico = icon::to_ico(&source).map_err(BuildError::Icon)?;
        std::fs::write(out.join(icon), ico).map_err(BuildError::Io)?;
    }
    std::fs::write(out.join(&resources.manifest_file), rc::manifest_for(resources))
        .map_err(BuildError::Io)?;
    std::fs::write(out.join(format!("{}.rc", resources.name)), rc::script(resources))
        .map_err(BuildError::Io)
}

/// Reads `rustnative.toml` into the resource description.
fn read_manifest(project: &Path) -> Result<rc::Resources, BuildError> {
    let path = project.join("rustnative.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| BuildError::Manifest(format!("{}: {error}", path.display())))?;
    let manifest: Manifest =
        toml::from_str(&text).map_err(|error| BuildError::Manifest(error.to_string()))?;
    let app = manifest.app;
    Ok(rc::Resources {
        display_name: app.display_name.clone().unwrap_or_else(|| app.name.clone()),
        company: app.publisher.clone().unwrap_or_else(|| app.name.clone()),
        description: app.description.clone().unwrap_or_else(|| app.name.clone()),
        icon_file: app.icon.as_ref().map(|_| format!("{}.ico", app.name)),
        icon_source: app.icon,
        manifest_file: format!("{}.manifest", app.name),
        version: app.version,
        name: app.name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("rustnative-build-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch folder");
        directory
    }

    fn write_project(directory: &Path, icon: bool) {
        let icon_line = if icon { "icon = \"icon.png\"\n" } else { "" };
        std::fs::write(
            directory.join("rustnative.toml"),
            format!(
                "[app]\nname = \"demo\"\nid = \"com.example.demo\"\ndisplay-name = \"Demo\"\n\
                 version = \"2.3.4\"\npublisher = \"CN=Example\"\n{icon_line}"
            ),
        )
        .expect("write rustnative.toml");
        if icon {
            let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            png.extend_from_slice(&13u32.to_be_bytes());
            png.extend_from_slice(b"IHDR");
            png.extend_from_slice(&64u32.to_be_bytes());
            png.extend_from_slice(&64u32.to_be_bytes());
            png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
            std::fs::write(directory.join("icon.png"), png).expect("write the icon");
        }
    }

    #[test]
    fn a_project_becomes_an_icon_a_manifest_and_a_script() {
        let project = scratch("prepare");
        let out = scratch("prepare-out");
        write_project(&project, true);
        let resources = read_manifest(&project).expect("readable");
        assert_eq!(resources.display_name, "Demo");
        assert_eq!(resources.company, "CN=Example");
        prepare(&project, &out, &resources).expect("prepared");

        let ico = std::fs::read(out.join("demo.ico")).expect("an icon was written");
        assert_eq!(&ico[..4], &[0, 0, 1, 0], "a real ICO header");
        let manifest = std::fs::read_to_string(out.join("demo.manifest")).expect("a manifest");
        assert!(manifest.contains("PerMonitorV2"));
        let script = std::fs::read_to_string(out.join("demo.rc")).expect("a script");
        assert!(script.contains("1 ICON \"demo.ico\""), "{script}");
        assert!(script.contains("FILEVERSION 2,3,4,0"), "{script}");
    }

    #[test]
    fn a_project_without_an_icon_still_gets_a_manifest_and_a_version() {
        let project = scratch("no-icon");
        let out = scratch("no-icon-out");
        write_project(&project, false);
        let resources = read_manifest(&project).expect("readable");
        prepare(&project, &out, &resources).expect("prepared");
        assert!(!out.join("demo.ico").exists());
        assert!(out.join("demo.manifest").is_file());
        assert!(std::fs::read_to_string(out.join("demo.rc")).unwrap().contains("VERSIONINFO"));
    }

    #[test]
    fn a_missing_manifest_is_reported_rather_than_guessed() {
        let project = scratch("no-manifest");
        assert!(matches!(read_manifest(&project), Err(BuildError::Manifest(_))));
    }
}
