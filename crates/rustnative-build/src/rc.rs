//! The resource script that carries an executable's icon, manifest, and
//! version information.
//!
//! Written as text and handed to the Windows SDK's `rc.exe`, which is the
//! only supported way to produce the `.res` the linker embeds.

use std::fmt::Write as _;

use crate::manifest;

/// What goes into the resource script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resources {
    /// The executable's name, as `VERSIONINFO`'s `InternalName`.
    pub name: String,
    /// The name people see.
    pub display_name: String,
    /// `major.minor.patch`.
    pub version: String,
    /// Who publishes it.
    pub company: String,
    /// One sentence about it.
    pub description: String,
    /// The `.ico` beside the script, if there is one.
    pub icon_file: Option<String>,
    /// Where that icon comes from in the project, which may be a `.png`.
    pub icon_source: Option<std::path::PathBuf>,
    /// The manifest file beside the script.
    pub manifest_file: String,
}

/// The `.rc` text for `resources`.
///
/// Paths are written relative and quoted; `rc.exe` resolves them against
/// the script's own folder, which is where the build script puts them.
#[must_use]
pub fn script(resources: &Resources) -> String {
    let version = comma_version(&resources.version);
    let dotted = dotted_version(&resources.version);
    let mut script = String::new();
    if let Some(icon) = &resources.icon_file {
        // Id 1: the first icon in an executable is the one Explorer, the
        // taskbar, and Alt+Tab use.
        let _ = write!(script, "1 ICON \"{}\"\n\n", escape(icon));
    }
    // 1 is `CREATEPROCESS_MANIFEST_RESOURCE_ID`, the manifest an executable
    // is activated with; 24 is `RT_MANIFEST`.
    let _ = write!(script, "1 24 \"{}\"\n\n", escape(&resources.manifest_file));
    let _ = write!(
        script,
        "1 VERSIONINFO\n\
         FILEVERSION {version}\n\
         PRODUCTVERSION {version}\n\
         FILEOS 0x4L\n\
         FILETYPE 0x1L\n\
         BEGIN\n\
         \x20   BLOCK \"StringFileInfo\"\n\
         \x20   BEGIN\n\
         \x20       BLOCK \"040904B0\"\n\
         \x20       BEGIN\n\
         \x20           VALUE \"CompanyName\", \"{company}\"\n\
         \x20           VALUE \"FileDescription\", \"{description}\"\n\
         \x20           VALUE \"FileVersion\", \"{dotted}\"\n\
         \x20           VALUE \"InternalName\", \"{name}\"\n\
         \x20           VALUE \"OriginalFilename\", \"{name}.exe\"\n\
         \x20           VALUE \"ProductName\", \"{product}\"\n\
         \x20           VALUE \"ProductVersion\", \"{dotted}\"\n\
         \x20       END\n\
         \x20   END\n\
         \x20   BLOCK \"VarFileInfo\"\n\
         \x20   BEGIN\n\
         \x20       VALUE \"Translation\", 0x409, 1200\n\
         \x20   END\n\
         END\n",
        company = escape(&resources.company),
        description = escape(&resources.description),
        name = escape(&resources.name),
        product = escape(&resources.display_name),
    );
    script
}

/// The manifest that goes with this script.
#[must_use]
pub fn manifest_for(resources: &Resources) -> String {
    manifest::application_manifest(&resources.display_name, &resources.version)
}

/// `1.2.3` as `rc.exe`'s comma-separated four-part form.
fn comma_version(version: &str) -> String {
    parts(version).map(|part| part.to_string()).collect::<Vec<_>>().join(",")
}

/// `1.2.3` as the four-part string a person reads in the file's properties.
fn dotted_version(version: &str) -> String {
    parts(version).map(|part| part.to_string()).collect::<Vec<_>>().join(".")
}

fn parts(version: &str) -> impl Iterator<Item = u16> {
    version
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(4)
}

/// Escapes what a quoted `.rc` string cannot hold literally.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\"\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources() -> Resources {
        Resources {
            name: "demo".to_owned(),
            display_name: "Demo \"App\"".to_owned(),
            version: "1.2.3".to_owned(),
            company: "Example \\ Co".to_owned(),
            description: "A demo".to_owned(),
            icon_file: Some("demo.ico".to_owned()),
            icon_source: Some(std::path::PathBuf::from("icon.png")),
            manifest_file: "demo.manifest".to_owned(),
        }
    }

    #[test]
    fn the_script_carries_the_icon_the_manifest_and_the_version() {
        let script = script(&resources());
        assert!(script.starts_with("1 ICON \"demo.ico\""), "{script}");
        assert!(script.contains("1 24 \"demo.manifest\""), "the manifest is RT_MANIFEST id 1");
        assert!(script.contains("FILEVERSION 1,2,3,0"), "{script}");
        assert!(script.contains("VALUE \"FileVersion\", \"1.2.3.0\""), "{script}");
        assert!(script.contains("VALUE \"OriginalFilename\", \"demo.exe\""), "{script}");
    }

    #[test]
    fn quotes_and_backslashes_cannot_end_a_string_early() {
        let script = script(&resources());
        assert!(script.contains(r#"VALUE "ProductName", "Demo ""App""""#), "{script}");
        assert!(script.contains(r#"VALUE "CompanyName", "Example \\ Co""#), "{script}");
    }

    #[test]
    fn an_application_without_an_icon_still_gets_a_manifest() {
        let mut resources = resources();
        resources.icon_file = None;
        let script = script(&resources);
        assert!(!script.contains("ICON"), "{script}");
        assert!(script.contains("1 24 "), "{script}");
    }
}
