//! Linux packages (Milestone 34): what a Linux desktop installs.
//!
//! - a **`.deb`** — Debian, Ubuntu, Kali, Mint, and the rest of the Debian
//!   family install it with `apt install ./app.deb`, which pulls in the
//!   GTK 4 and libsoup it depends on;
//! - an **`.rpm`** — Fedora, RHEL and its rebuilds, and openSUSE install it
//!   with `dnf install ./app.rpm` (or `zypper`), which pulls in GTK 4 and
//!   libsoup 3 the same way ([`rpm`]);
//! - a **pacman package** — Arch Linux, Manjaro, and EndeavourOS install it
//!   with `pacman -U app.pkg.tar` ([`pacman`]);
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
//! uncompressed — `dpkg` accepts an uncompressed `data.tar`, and `pacman` an
//! uncompressed package — for the same reason the ZIP is: nothing depends on
//! a compressor's exact output. Where a format insists on gzip (the RPM
//! payload, pacman's `.MTREE`), the stream holds stored blocks
//! ([`gzip_stored`]).

pub mod pacman;
pub mod rpm;

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

/// A gzip stream (RFC 1952) of `data` in stored deflate blocks: valid for
/// every reader, identical on every machine, and needing no compressor.
#[must_use]
pub fn gzip_stored(data: &[u8]) -> Vec<u8> {
    // No name, no time, "Unix".
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
    let mut blocks = data.chunks(usize::from(u16::MAX)).peekable();
    if blocks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let length = u16::try_from(block.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
    // ISIZE is the length modulo 2^32.
    let length = u32::try_from(data.len() as u64 & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    out.extend_from_slice(&length.to_le_bytes());
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

/// The RPM name for this build's architecture.
#[must_use]
pub fn rpm_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "i686",
        "arm" => "armv7hl",
        other => other,
    }
}

/// The pacman name for this build's architecture.
#[must_use]
pub fn pacman_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "i686",
        "arm" => "armv7h",
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
///
/// It carries what `appstreamcli validate` requires of a desktop
/// application — a description, and a date on each release (the package's
/// own timestamp, so the file stays reproducible) — because GNOME Software
/// and Discover leave out a component that fails validation.
#[must_use]
pub fn metainfo(config: &Config) -> String {
    let app = &config.app;
    let summary = app.description.as_deref().unwrap_or(&app.display_name);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<component type=\"desktop-application\">\n  <id>{id}</id>\n  <name>{name}</name>\n  <summary>{summary}</summary>\n  <description>\n    <p>{summary}</p>\n  </description>\n  <metadata_license>CC0-1.0</metadata_license>\n  <launchable type=\"desktop-id\">{id}.desktop</launchable>\n  <releases>\n    <release version=\"{version}\" date=\"{date}\"/>\n  </releases>\n</component>\n",
        id = xml_escape(&app.id),
        name = xml_escape(&app.display_name),
        summary = xml_escape(summary),
        version = xml_escape(&app.version),
        date = iso_date(timestamp()),
    )
}

/// `seconds` since the epoch as an ISO 8601 date (`YYYY-MM-DD`, UTC).
fn iso_date(seconds: u64) -> String {
    // Howard Hinnant's days-to-civil, on days since 1970-01-01.
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX / 2) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
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

/// What the RPM and pacman packages need from the host: the libraries the
/// `.deb` names, in those distributions' package names.
const RPM_REQUIRES: &[(&str, Option<&str>)] = &[("gtk4", Some("4.14")), ("libsoup3", None)];
const PACMAN_DEPENDS: &[(&str, Option<&str>)] =
    &[("gtk4", Some("4.14")), ("libsoup3", None), ("glibc", None)];

/// Builds the `.rpm` and returns its path.
///
/// # Errors
///
/// As [`build_deb`].
pub fn build_rpm(
    output: &Path,
    executable: &Path,
    icon: Option<&Path>,
    config: &Config,
) -> Result<PathBuf> {
    let binary = read(executable, "the built executable")?;
    let icon = icon.map(|icon| read(icon, "the icon")).transpose()?;
    let files = installed_files(config, &binary, icon.as_deref(), "usr/");
    let name = package_name(&config.app.name);
    let package = rpm::rpm(
        &rpm::Metadata {
            name: &name,
            version: &config.app.version,
            release: "1",
            summary: config.app.description.as_deref().unwrap_or(&config.app.display_name),
            packager: config.app.publisher.as_deref().unwrap_or("unknown"),
            arch: rpm_architecture(),
            requires: RPM_REQUIRES,
        },
        &files,
        timestamp(),
    );
    let path = output.join(format!("{name}-{}-1.{}.rpm", config.app.version, rpm_architecture()));
    write(&path, &package)?;
    Ok(path)
}

/// Builds the pacman package and returns its path.
///
/// # Errors
///
/// As [`build_deb`].
pub fn build_pacman(
    output: &Path,
    executable: &Path,
    icon: Option<&Path>,
    config: &Config,
) -> Result<PathBuf> {
    let binary = read(executable, "the built executable")?;
    let icon = icon.map(|icon| read(icon, "the icon")).transpose()?;
    let files = installed_files(config, &binary, icon.as_deref(), "usr/");
    let name = package_name(&config.app.name);
    let package = pacman::package(
        &pacman::Metadata {
            name: &name,
            version: &config.app.version,
            release: "1",
            description: config.app.description.as_deref().unwrap_or(&config.app.display_name),
            packager: config.app.publisher.as_deref().unwrap_or("Unknown Packager"),
            arch: pacman_architecture(),
            depends: PACMAN_DEPENDS,
        },
        &files,
        timestamp(),
    );
    let path =
        output.join(format!("{name}-{}-1-{}.pkg.tar", config.app.version, pacman_architecture()));
    write(&path, &package)?;
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
        let info = metainfo(&config());
        assert!(info.contains("<summary>Take notes &amp; keep them</summary>"));
        assert!(info.contains("<description>\n    <p>Take notes &amp; keep them</p>"));
        assert!(info.contains(&format!("date=\"{}\"", iso_date(timestamp()))), "{info}");
    }

    #[test]
    fn dates_are_iso_8601_in_utc() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(951_782_400), "2000-02-29");
        assert_eq!(iso_date(1_791_158_399), "2026-10-04");
        assert_eq!(iso_date(4_102_444_800), "2100-01-01");
    }

    /// Where `desktop-file-validate` is installed, the desktop entry passes
    /// it.
    #[test]
    fn desktop_file_utils_validates_the_entry() {
        let Some(tool) = which("desktop-file-validate") else {
            eprintln!("skipped: desktop-file-validate is not installed");
            return;
        };
        let (directory, _) = scratch("desktop");
        let path = directory.join("com.example.Notes.desktop");
        std::fs::write(&path, desktop_entry(&config())).expect("written");
        let output = std::process::Command::new(tool).arg(&path).output().expect("runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Where `appstreamcli` is installed, the metadata has none of the
    /// errors software centres check for before listing a component.
    #[test]
    fn appstream_itself_validates_the_metadata() {
        let Some(tool) = which("appstreamcli") else {
            eprintln!("skipped: appstreamcli is not installed");
            return;
        };
        let (directory, _) = scratch("metainfo");
        let path = directory.join("com.example.Notes.metainfo.xml");
        std::fs::write(&path, metainfo(&config())).expect("written");
        let output = std::process::Command::new(tool)
            .args(["validate", "--no-net"])
            .arg(&path)
            .output()
            .expect("appstreamcli runs");
        // An error (`E:`) is what makes a software centre drop the
        // component; a warning such as a missing homepage is the author's
        // to add, from information `rustnative.toml` does not carry.
        let report = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        assert!(!report.lines().any(|line| line.starts_with("E:")), "{report}");
        let _ = std::fs::remove_dir_all(&directory);
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

    #[test]
    fn a_stored_gzip_stream_carries_the_data_its_crc_and_its_length() {
        let data = vec![7_u8; 70_000];
        let stream = gzip_stored(&data);
        assert_eq!(&stream[..3], &[0x1f, 0x8b, 8]);
        // Two blocks: 65535 bytes, not final; then the rest, final.
        assert_eq!(stream[10], 0);
        assert_eq!(&stream[11..15], &[0xff, 0xff, 0, 0]);
        let second = 10 + 5 + 65_535;
        assert_eq!(stream[second], 1);
        assert_eq!(stream.len(), second + 5 + (70_000 - 65_535) + 8);
        let tail = &stream[stream.len() - 8..];
        assert_eq!(&tail[..4], &crc32fast::hash(&data).to_le_bytes());
        assert_eq!(&tail[4..], &70_000_u32.to_le_bytes());
        // An empty stream still has its one, final, empty block.
        assert_eq!(gzip_stored(&[]).len(), 10 + 5 + 8);
    }

    /// Where `gzip` is installed (every distribution), it decompresses the
    /// stream to the data.
    #[test]
    fn gzip_itself_reads_a_stored_stream() {
        use std::io::Write as _;

        let Some(tool) = which("gzip") else {
            eprintln!("skipped: gzip is not installed");
            return;
        };
        let data = (0..200_000_u32)
            .map(|value| u8::try_from(value % 251).expect("under 256"))
            .collect::<Vec<_>>();
        let mut child = std::process::Command::new(tool)
            .args(["-d", "-c"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("gzip runs");
        child.stdin.take().expect("stdin").write_all(&gzip_stored(&data)).expect("written");
        let output = child.wait_with_output().expect("gzip finishes");
        assert!(output.status.success());
        assert_eq!(output.stdout, data);
    }

    /// A scratch directory holding a stand-in executable.
    fn scratch(what: &str) -> (PathBuf, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("rustnative-{what}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        let executable = directory.join("notes");
        std::fs::write(&executable, b"\x7fELF").expect("written");
        (directory, executable)
    }

    /// Where `dpkg-deb` is installed (the Debian family), it reads the
    /// package's control file and file list.
    #[test]
    fn dpkg_itself_reads_the_deb() {
        let Some(tool) = which("dpkg-deb") else {
            eprintln!("skipped: dpkg-deb is not installed");
            return;
        };
        let (directory, executable) = scratch("deb");
        let path = build_deb(&directory, &executable, None, &config()).expect("built");
        let run = |flag: &str| {
            let output =
                std::process::Command::new(&tool).arg(flag).arg(&path).output().expect("runs");
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        assert!(run("-I").contains("Package: notes"));
        assert!(run("-c").contains("./usr/bin/notes"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_rpm_and_pacman_packages_are_named_as_their_distributions_name_them() {
        let (directory, executable) = scratch("native");
        let config = config();
        let rpm = build_rpm(&directory, &executable, None, &config).expect("built");
        let expected = format!("notes-{}-1.{}.rpm", config.app.version, rpm_architecture());
        assert_eq!(rpm.file_name().and_then(|name| name.to_str()), Some(expected.as_str()));
        let pacman = build_pacman(&directory, &executable, None, &config).expect("built");
        let expected = format!("notes-{}-1-{}.pkg.tar", config.app.version, pacman_architecture());
        assert_eq!(pacman.file_name().and_then(|name| name.to_str()), Some(expected.as_str()));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
