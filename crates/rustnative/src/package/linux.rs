//! Linux packages (Milestone 34): what a Linux desktop installs.
//!
//! - a **`.deb`** — Debian, Ubuntu, Kali, Mint, and the rest of the Debian
//!   family install it with `apt install ./app.deb`, which pulls in the
//!   GTK 4 and libsoup it depends on;
//! - a **tarball** — the same files under `<name>-<version>/`, for people
//!   who unpack and run, or a distribution's packager to start from;
//! - an **AppImage** — one self-contained file that runs on any recent
//!   distribution, built by `appimagetool` from an `AppDir`.
//!
//! Every package carries the desktop entry (`<id>.desktop`: the name the
//! launcher shows, the `%u` that hands a URL to the application, the URL
//! schemes it handles, and the window class that matches its windows to
//! it), the AppStream metadata software centres read, and the icon.
//!
//! The archives are reproducible: entries are sorted, every timestamp is
//! `SOURCE_DATE_EPOCH` (or zero), and owners are root. They are stored
//! uncompressed — `dpkg` accepts an uncompressed `data.tar` — for the same
//! reason the ZIP is: nothing depends on a compressor's exact output.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Result};

/// One file in an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Its path, relative, with `/` separators.
    pub path: String,
    /// Its contents.
    pub data: Vec<u8>,
    /// Whether it is executable.
    pub executable: bool,
}

/// The time every entry carries: `SOURCE_DATE_EPOCH`, the reproducible-
/// builds convention, or the epoch.
fn timestamp() -> u64 {
    std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|value| value.parse().ok()).unwrap_or(0)
}

/// An octal field of `width` bytes, NUL-terminated.
fn octal(value: u64, width: usize) -> Vec<u8> {
    let mut field = format!("{value:0digits$o}", digits = width - 1).into_bytes();
    field.push(0);
    field
}

/// A POSIX ustar archive of `files` (and the directories they are in),
/// sorted by path.
#[must_use]
pub fn tar(files: &[File]) -> Vec<u8> {
    let mut entries: Vec<(String, Option<&File>)> = Vec::new();
    for file in files {
        // Each parent directory, once, before what is in it.
        let mut prefix = String::new();
        for part in
            file.path.split('/').collect::<Vec<_>>().split_last().map_or(&[][..], |(_, dirs)| dirs)
        {
            prefix.push_str(part);
            prefix.push('/');
            if !entries.iter().any(|(path, _)| *path == prefix) {
                entries.push((prefix.clone(), None));
            }
        }
        entries.push((file.path.clone(), Some(file)));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.dedup_by(|left, right| left.0 == right.0);

    let time = timestamp();
    let mut out = Vec::new();
    for (path, file) in entries {
        let mut header = [0_u8; 512];
        let (name, prefix) = if path.len() <= 100 {
            (path.as_str(), "")
        } else {
            // ustar splits a long path at a `/` into prefix and name.
            let split = path[..path.len().min(155)].rfind('/').unwrap_or(0);
            (&path[split + 1..], &path[..split])
        };
        header[..name.len().min(100)].copy_from_slice(&name.as_bytes()[..name.len().min(100)]);
        let (mode, size, kind) = match file {
            Some(file) => {
                (if file.executable { 0o755 } else { 0o644 }, file.data.len() as u64, b'0')
            }
            None => (0o755, 0, b'5'),
        };
        header[100..108].copy_from_slice(&octal(mode, 8));
        header[108..116].copy_from_slice(&octal(0, 8));
        header[116..124].copy_from_slice(&octal(0, 8));
        header[124..136].copy_from_slice(&octal(size, 12));
        header[136..148].copy_from_slice(&octal(time, 12));
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[265..269].copy_from_slice(b"root");
        header[297..301].copy_from_slice(b"root");
        header[345..345 + prefix.len().min(155)]
            .copy_from_slice(&prefix.as_bytes()[..prefix.len().min(155)]);
        // The checksum is computed with its own field as spaces.
        header[148..156].copy_from_slice(b"        ");
        let checksum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
        header[148..155].copy_from_slice(&octal(checksum, 7));
        header[155] = b' ';
        out.extend_from_slice(&header);
        if let Some(file) = file {
            out.extend_from_slice(&file.data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
    }
    // Two zero blocks end the archive.
    out.resize(out.len() + 1024, 0);
    out
}

/// A `.deb`: an `ar` archive of `debian-binary`, `control.tar`, and
/// `data.tar`, in that order (`deb(5)`).
#[must_use]
pub fn deb(control: &[File], data: &[File]) -> Vec<u8> {
    let time = timestamp();
    let mut out = b"!<arch>\n".to_vec();
    for (name, contents) in [
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar", tar(control)),
        ("data.tar", tar(data)),
    ] {
        let header =
            format!("{name:<16}{time:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 100_644, contents.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&contents);
        if contents.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    out
}

/// The Debian architecture this build is for.
#[must_use]
pub fn debian_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        "riscv64" => "riscv64",
        other => other,
    }
}

/// The desktop entry (`<id>.desktop`).
#[must_use]
pub fn desktop_entry(config: &Config) -> String {
    let app = &config.app;
    let mut entry = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.5\nName={}\nExec={} %u\nIcon={}\nTerminal=false\nStartupNotify=true\nStartupWMClass={}\nCategories=Utility;\n",
        app.display_name, app.name, app.id, app.id
    );
    if let Some(description) = &app.description {
        let _ = writeln!(entry, "Comment={description}");
    }
    if !app.url_schemes.is_empty() {
        entry.push_str("MimeType=");
        for scheme in &app.url_schemes {
            let _ = write!(entry, "x-scheme-handler/{scheme};");
        }
        entry.push('\n');
    }
    entry
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The AppStream metadata (`<id>.metainfo.xml`) software centres read.
#[must_use]
pub fn metainfo(config: &Config) -> String {
    let app = &config.app;
    let summary = app.description.as_deref().unwrap_or(&app.display_name);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<component type=\"desktop-application\">\n  <id>{id}</id>\n  <name>{name}</name>\n  <summary>{summary}</summary>\n  <metadata_license>CC0-1.0</metadata_license>\n  <launchable type=\"desktop-id\">{id}.desktop</launchable>\n  <releases>\n    <release version=\"{version}\"/>\n  </releases>\n</component>\n",
        id = xml_escape(&app.id),
        name = xml_escape(&app.display_name),
        summary = xml_escape(summary),
        version = xml_escape(&app.version),
    )
}

/// The Debian package name: lower case, `[a-z0-9+.-]`.
fn package_name(name: &str) -> String {
    name.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "+.-".contains(c) { c } else { '-' })
        .collect()
}

/// The `control` file.
#[must_use]
pub fn control(config: &Config, installed_kib: u64) -> String {
    let app = &config.app;
    let summary = app.description.as_deref().unwrap_or(&app.display_name);
    format!(
        "Package: {}\nVersion: {}\nArchitecture: {}\nMaintainer: {}\nInstalled-Size: {installed_kib}\nDepends: libgtk-4-1 (>= 4.14), libsoup-3.0-0, libc6\nSection: misc\nPriority: optional\nDescription: {summary}\n",
        package_name(&app.name),
        app.version,
        debian_architecture(),
        app.publisher.as_deref().unwrap_or("unknown <unknown@localhost>"),
    )
}

/// The files the application installs, under `prefix` (`usr/` in a
/// `.deb`, nothing in a tarball), with the icon if it has one.
fn installed_files(
    config: &Config,
    executable: &[u8],
    icon: Option<&[u8]>,
    prefix: &str,
) -> Vec<File> {
    let app = &config.app;
    let mut files = vec![
        File {
            path: format!("{prefix}bin/{}", app.name),
            data: executable.to_vec(),
            executable: true,
        },
        File {
            path: format!("{prefix}share/applications/{}.desktop", app.id),
            data: desktop_entry(config).into_bytes(),
            executable: false,
        },
        File {
            path: format!("{prefix}share/metainfo/{}.metainfo.xml", app.id),
            data: metainfo(config).into_bytes(),
            executable: false,
        },
    ];
    if let Some(icon) = icon {
        let (folder, extension) = icon_slot(icon);
        files.push(File {
            path: format!("{prefix}share/icons/hicolor/{folder}/apps/{}.{extension}", app.id),
            data: icon.to_vec(),
            executable: false,
        });
    }
    files
}

/// Where an icon goes in the hicolor theme: a PNG by its width, an SVG as
/// scalable.
fn icon_slot(icon: &[u8]) -> (String, &'static str) {
    if icon.starts_with(b"\x89PNG\r\n\x1a\n") && icon.len() >= 24 {
        let width = u32::from_be_bytes([icon[16], icon[17], icon[18], icon[19]]);
        (format!("{width}x{width}"), "png")
    } else {
        ("scalable".to_owned(), "svg")
    }
}

fn write(path: &Path, data: &[u8]) -> Result<()> {
    std::fs::write(path, data)
        .map_err(|cause| Error::Io { what: format!("write {}", path.display()), cause })
}

fn read(path: &Path, what: &str) -> Result<Vec<u8>> {
    std::fs::read(path)
        .map_err(|cause| Error::Io { what: format!("read {what} at {}", path.display()), cause })
}

/// Builds the `.deb` and returns its path.
///
/// # Errors
///
/// The executable or icon could not be read, or the package written.
pub fn build_deb(
    output: &Path,
    executable: &Path,
    icon: Option<&Path>,
    config: &Config,
) -> Result<PathBuf> {
    let binary = read(executable, "the built executable")?;
    let icon = icon.map(|icon| read(icon, "the icon")).transpose()?;
    let data = installed_files(config, &binary, icon.as_deref(), "./usr/");
    let installed: u64 = data.iter().map(|file| file.data.len() as u64).sum::<u64>().div_ceil(1024);
    let control_files = [File {
        path: "./control".to_owned(),
        data: control(config, installed).into_bytes(),
        executable: false,
    }];
    let path = output.join(format!(
        "{}_{}_{}.deb",
        package_name(&config.app.name),
        config.app.version,
        debian_architecture()
    ));
    write(&path, &deb(&control_files, &data))?;
    Ok(path)
}

/// Builds the tarball and returns its path.
///
/// # Errors
///
/// As [`build_deb`].
pub fn build_tar(
    output: &Path,
    executable: &Path,
    icon: Option<&Path>,
    config: &Config,
) -> Result<PathBuf> {
    let binary = read(executable, "the built executable")?;
    let icon = icon.map(|icon| read(icon, "the icon")).transpose()?;
    let root = format!("{}-{}/", config.app.name, config.app.version);
    let files = installed_files(config, &binary, icon.as_deref(), &root);
    let path = output.join(format!(
        "{}-{}-linux-{}.tar",
        config.app.name,
        config.app.version,
        std::env::consts::ARCH
    ));
    write(&path, &tar(&files))?;
    Ok(path)
}

/// Lays out an `AppDir` and runs `appimagetool` on it, returning the
/// AppImage's path.
///
/// # Errors
///
/// `appimagetool` is not installed or failed, or a file could not be
/// written.
pub fn build_appimage(
    output: &Path,
    executable: &Path,
    icon: Option<&Path>,
    config: &Config,
) -> Result<PathBuf> {
    let tool = which("appimagetool").ok_or_else(|| Error::ToolMissing {
        tool: "appimagetool",
        hint:
            "download it from https://github.com/AppImage/appimagetool/releases and put it on PATH"
                .to_owned(),
        cause: None,
    })?;
    let binary = read(executable, "the built executable")?;
    let icon = icon.map(|icon| read(icon, "the icon")).transpose()?;
    let appdir = output.join(format!("{}.AppDir", config.app.name));
    let _ = std::fs::remove_dir_all(&appdir);
    let mut files = installed_files(config, &binary, icon.as_deref(), "usr/");
    // The AppDir's root holds the desktop entry, the icon, and `AppRun`.
    files.push(File {
        path: format!("{}.desktop", config.app.id),
        data: desktop_entry(config).into_bytes(),
        executable: false,
    });
    if let Some(icon) = &icon {
        files.push(File {
            path: format!("{}.{}", config.app.id, icon_slot(icon).1),
            data: icon.clone(),
            executable: false,
        });
    }
    files.push(File {
        path: "AppRun".to_owned(),
        data: format!(
            "#!/bin/sh\nexec \"$(dirname \"$(readlink -f \"$0\")\")/usr/bin/{}\" \"$@\"\n",
            config.app.name
        )
        .into_bytes(),
        executable: true,
    });
    for file in &files {
        let path = appdir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|cause| Error::Io {
                what: format!("create {}", parent.display()),
                cause,
            })?;
        }
        write(&path, &file.data)?;
        #[cfg(unix)]
        if file.executable {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(
                |cause| Error::Io { what: format!("make {} executable", path.display()), cause },
            )?;
        }
    }
    let path = output.join(format!(
        "{}-{}-{}.AppImage",
        config.app.name,
        config.app.version,
        std::env::consts::ARCH
    ));
    let status = std::process::Command::new(&tool)
        .arg(&appdir)
        .arg(&path)
        .env("ARCH", std::env::consts::ARCH)
        .status()
        .map_err(|cause| Error::Io { what: "run appimagetool".to_owned(), cause })?;
    if !status.success() {
        return Err(Error::Io {
            what: "build the AppImage".to_owned(),
            cause: std::io::Error::other(format!("appimagetool exited with {status}")),
        });
    }
    Ok(path)
}

/// `program` on `PATH`.
#[must_use]
pub fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|dir| Path::new(dir).join(program))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut config = Config::template("notes");
        config.app.id = "com.example.Notes".to_owned();
        config.app.description = Some("Take notes & keep them".to_owned());
        config.app.url_schemes = vec!["notes".to_owned()];
        config
    }

    #[test]
    fn the_desktop_entry_hands_urls_to_the_application_and_names_its_windows() {
        let entry = desktop_entry(&config());
        assert!(entry.starts_with("[Desktop Entry]\n"));
        for line in [
            "Exec=notes %u",
            "Icon=com.example.Notes",
            "StartupWMClass=com.example.Notes",
            "MimeType=x-scheme-handler/notes;",
        ] {
            assert!(entry.lines().any(|candidate| candidate == line), "{line} in\n{entry}");
        }
        assert!(metainfo(&config()).contains("<summary>Take notes &amp; keep them</summary>"));
    }

    #[test]
    fn a_tar_header_is_valid_ustar_and_files_are_block_aligned() {
        let archive =
            tar(&[File { path: "a/b.txt".to_owned(), data: b"hello".to_vec(), executable: false }]);
        // A directory entry, the file's header and one data block, and the end.
        assert_eq!(archive.len(), 512 + 512 + 512 + 1024);
        assert_eq!(&archive[..2], b"a/");
        assert_eq!(archive[156], b'5');
        let file = &archive[512..1024];
        assert_eq!(&file[..7], b"a/b.txt");
        assert_eq!(&file[257..263], b"ustar\0");
        let stored = u64::from_str_radix(std::str::from_utf8(&file[148..154]).expect("ascii"), 8)
            .expect("octal");
        let mut blank = file.to_vec();
        blank[148..156].copy_from_slice(b"        ");
        assert_eq!(
            stored,
            blank.iter().map(|byte| u64::from(*byte)).sum::<u64>(),
            "the header checksum"
        );
        assert_eq!(&archive[1024..1029], b"hello");
        assert_eq!(
            tar(&[File { path: "a/b.txt".to_owned(), data: b"hello".to_vec(), executable: false }]),
            archive,
            "reproducible"
        );
    }

    #[test]
    fn a_deb_is_an_ar_archive_of_the_three_members() {
        let package = deb(
            &[File {
                path: "./control".to_owned(),
                data: b"Package: x\n".to_vec(),
                executable: false,
            }],
            &[],
        );
        assert!(package.starts_with(b"!<arch>\ndebian-binary   "));
        let text = String::from_utf8_lossy(&package);
        let order = ["debian-binary", "control.tar", "data.tar"]
            .map(|member| text.find(member).expect(member));
        assert!(order[0] < order[1] && order[1] < order[2], "dpkg reads the members in this order");
        assert!(control(&config(), 12).contains("Package: notes\nVersion: "));
    }
}
