//! MSIX packages: the manifest, the layout, and `makeappx`.
//!
//! An MSIX is a folder with an `AppxManifest.xml` in it, packed by the
//! Windows SDK's `makeappx`. What `rustnative` contributes is the manifest — built
//! from the same `rustnative.toml` the application's own identity comes from, so
//! the package's identity, the state store's folder, and the
//! single-instance mutex cannot drift apart — and the layout around it.
//!
//! The application runs as a full-trust desktop application
//! (`runFullTrust`), which is what a framework built on Win32 controls
//! needs, and each URL scheme from Milestone 30 becomes a `uap:Protocol`
//! extension, so a `myapp://` link launches it.

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Result};

/// The images an MSIX names. They are a package's visual identity, so a
/// real application replaces them; a generated placeholder keeps
/// `makeappx` happy until it does.
pub const LOGOS: [(&str, &str); 2] = [
    ("Square44x44Logo", "assets/Square44x44Logo.png"),
    ("Square150x150Logo", "assets/Square150x150Logo.png"),
];

/// The `AppxManifest.xml` for `config`.
#[must_use]
pub fn manifest(config: &Config) -> String {
    let app = &config.app;
    let publisher = app.publisher.clone().unwrap_or_else(|| format!("CN={}", app.name));
    let description = app.description.clone().unwrap_or_else(|| app.display_name.clone());
    let protocols = app.url_schemes.iter().fold(String::new(), |mut protocols, scheme| {
        use std::fmt::Write as _;

        let _ = write!(
            protocols,
            "      <uap:Extension Category=\"windows.protocol\">\n\
             \x20       <uap:Protocol Name=\"{}\" />\n\
             \x20     </uap:Extension>\n",
            escape(scheme)
        );
        protocols
    });
    // Device capabilities are a separate element, after the others.
    let (device, general): (Vec<&String>, Vec<&String>) = config
        .package
        .capabilities
        .iter()
        .partition(|name| DEVICE_CAPABILITIES.contains(&name.as_str()));
    let capabilities = general
        .into_iter()
        .map(|name| ("Capability", name))
        .chain(device.into_iter().map(|name| ("DeviceCapability", name)))
        .fold(String::new(), |mut out, (element, name)| {
            use std::fmt::Write as _;

            let _ = writeln!(out, "    <{element} Name=\"{}\" />", escape(name));
            out
        });
    let extensions = if protocols.is_empty() {
        String::new()
    } else {
        format!("      <Extensions>\n{protocols}      </Extensions>\n")
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities">
  <Identity Name="{identity}" Publisher="{publisher}" Version="{version}" ProcessorArchitecture="{architecture}" />
  <Properties>
    <DisplayName>{display_name}</DisplayName>
    <PublisherDisplayName>{publisher_display}</PublisherDisplayName>
    <Description>{description}</Description>
    <Logo>{store_logo}</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
{capabilities}  </Capabilities>
  <Applications>
    <Application Id="App" Executable="{executable}" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="{display_name}"
        Description="{description}"
        Square150x150Logo="{square150}"
        Square44x44Logo="{square44}"
        BackgroundColor="transparent" />
{extensions}    </Application>
  </Applications>
</Package>
"#,
        identity = escape(&app.id),
        publisher = escape(&publisher),
        version = four_part(&app.version),
        architecture = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" },
        display_name = escape(&app.display_name),
        publisher_display = escape(publisher_display_name(&publisher)),
        description = escape(&description),
        store_logo = LOGOS[1].1,
        executable = escape(&format!("{}.exe", app.name)),
        square150 = LOGOS[1].1,
        square44 = LOGOS[0].1,
    )
}

/// Capabilities MSIX declares as `DeviceCapability` rather than
/// `Capability`.
const DEVICE_CAPABILITIES: [&str; 6] =
    ["webcam", "microphone", "location", "bluetooth", "proximity", "serialcommunication"];

/// The `.appinstaller` file for a package published at `package_url`:
/// App Installer installs from it and checks `<its url>` on every launch.
#[must_use]
pub fn appinstaller(config: &Config, appinstaller_url: &str, package_url: &str) -> String {
    let app = &config.app;
    let publisher = app.publisher.clone().unwrap_or_else(|| format!("CN={}", app.name));
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<AppInstaller xmlns="http://schemas.microsoft.com/appx/appinstaller/2018" Version="{version}" Uri="{uri}">
  <MainPackage Name="{identity}" Publisher="{publisher}" Version="{version}" ProcessorArchitecture="{architecture}" Uri="{package}" />
  <UpdateSettings>
    <OnLaunch HoursBetweenUpdateChecks="0" />
  </UpdateSettings>
</AppInstaller>
"#,
        version = four_part(&app.version),
        uri = escape(appinstaller_url),
        identity = escape(&app.id),
        publisher = escape(&publisher),
        architecture = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" },
        package = escape(package_url),
    )
}

/// The human-readable part of an X.500 publisher name: `CN=Example Ltd,
/// O=Example` shows as `Example Ltd`.
fn publisher_display_name(publisher: &str) -> &str {
    publisher
        .split(',')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("CN="))
        .unwrap_or(publisher)
}

/// A package version is four numbers, and its last part must be zero.
fn four_part(version: &str) -> String {
    let numbers = version
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(3)
        .map(|part| part.to_string())
        .collect::<Vec<_>>()
        .join(".");
    format!("{numbers}.0")
}

/// Lays out everything the package holds in `layout`, ready for
/// `makeappx`: the executable, the manifest, and the logos.
///
/// # Errors
///
/// [`Error::Io`] if the layout could not be written.
pub fn lay_out(
    layout: &Path,
    executable: &Path,
    config: &Config,
    icon: Option<&Path>,
) -> Result<()> {
    let io = |what: &str, cause: std::io::Error| Error::Io { what: what.to_owned(), cause };
    std::fs::create_dir_all(layout.join("assets"))
        .map_err(|cause| io("create the package layout", cause))?;
    let target = layout.join(format!("{}.exe", config.app.name));
    std::fs::copy(executable, &target)
        .map_err(|cause| io(&format!("copy {}", executable.display()), cause))?;
    std::fs::write(layout.join("AppxManifest.xml"), manifest(config))
        .map_err(|cause| io("write AppxManifest.xml", cause))?;

    let logo = match icon {
        Some(path) if path.extension().is_some_and(|extension| extension == "png") => {
            std::fs::read(path).map_err(|cause| io("read the icon", cause))?
        }
        // No icon, or one that is not a PNG: a placeholder, so the package
        // builds and the application can replace it later.
        _ => placeholder_png(),
    };
    for (_, path) in LOGOS {
        std::fs::write(layout.join(path), &logo)
            .map_err(|cause| io(&format!("write {path}"), cause))?;
    }
    Ok(())
}

/// A 1x1 opaque PNG, built byte by byte so that packaging needs no image
/// library: signature, `IHDR`, an `IDAT` holding one uncompressed zlib
/// block, and `IEND`.
#[must_use]
pub fn placeholder_png() -> Vec<u8> {
    fn chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(data.len()).unwrap_or(0).to_be_bytes().to_vec();
        out.extend_from_slice(&kind);
        out.extend_from_slice(data);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&kind);
        hasher.update(data);
        out.extend_from_slice(&hasher.finalize().to_be_bytes());
        out
    }

    // One row: a filter byte, then one RGBA pixel.
    let raw = [0u8, 0x2b, 0x2b, 0x2b, 0xff];
    let mut zlib = vec![0x78, 0x01]; // deflate, 32 KiB window, no dictionary
    zlib.push(0x01); // a final, stored block
    zlib.extend_from_slice(&u16::try_from(raw.len()).unwrap_or(0).to_le_bytes());
    zlib.extend_from_slice(&(!u16::try_from(raw.len()).unwrap_or(0)).to_le_bytes());
    zlib.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut header = Vec::new();
    header.extend_from_slice(&1u32.to_be_bytes()); // width
    header.extend_from_slice(&1u32.to_be_bytes()); // height
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&chunk(*b"IHDR", &header));
    png.extend_from_slice(&chunk(*b"IDAT", &zlib));
    png.extend_from_slice(&chunk(*b"IEND", &[]));
    png
}

/// Packs `layout` into `output` with the SDK's `makeappx`.
///
/// # Errors
///
/// [`Error::ToolMissing`] if the SDK is not installed, or
/// [`Error::ToolFailed`] with `makeappx`'s own message.
pub fn pack(makeappx: &Path, layout: &Path, output: &Path) -> Result<()> {
    let result = std::process::Command::new(makeappx)
        .arg("pack")
        .arg("/d")
        .arg(layout)
        .arg("/p")
        .arg(output)
        .arg("/o")
        .output()
        .map_err(|cause| Error::ToolMissing {
            tool: "makeappx",
            hint: "install the Windows SDK".to_owned(),
            cause: Some(cause.to_string()),
        })?;
    if result.status.success() {
        return Ok(());
    }
    eprintln!("{}", String::from_utf8_lossy(&result.stdout).trim());
    Err(Error::ToolFailed { tool: "makeappx", code: result.status.code() })
}

/// Where the package's layout is built, inside the output folder.
#[must_use]
pub fn layout_directory(output: &Path, name: &str) -> PathBuf {
    output.join(format!("{name}-msix-layout"))
}

fn escape(text: &str) -> String {
    crate::package::escape_xml(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::App;

    fn config(schemes: &[&str]) -> Config {
        Config {
            app: App {
                name: "demo".to_owned(),
                id: "com.example.demo".to_owned(),
                display_name: "Demo & Co".to_owned(),
                version: "1.2.3".to_owned(),
                publisher: Some("CN=Example Ltd, O=Example".to_owned()),
                description: Some("A demo".to_owned()),
                icon: None,
                url_schemes: schemes.iter().map(|scheme| (*scheme).to_owned()).collect(),
            },
            resources: std::collections::BTreeMap::new(),
            i18n: crate::config::I18n::default(),
            style: None,
            package: crate::config::Package::default(),
            update: None,
            web: None,
            android: None,
        }
    }

    #[test]
    fn declared_capabilities_are_in_the_manifest_devices_last() {
        let mut declared = config(&[]);
        declared.package.capabilities = vec!["webcam".into(), "internetClient".into()];
        let manifest = manifest(&declared);
        let general = manifest.find(r#"<Capability Name="internetClient" />"#).expect(&manifest);
        let device = manifest.find(r#"<DeviceCapability Name="webcam" />"#).expect(&manifest);
        assert!(general < device, "MSIX requires device capabilities after the others: {manifest}");
    }

    #[test]
    fn the_appinstaller_points_at_itself_and_the_package() {
        let text = appinstaller(
            &config(&[]),
            "https://example.com/demo.appinstaller",
            "https://example.com/demo-1.2.3.msix",
        );
        assert!(text.contains(r#"Uri="https://example.com/demo.appinstaller""#), "{text}");
        assert!(
            text.contains(r#"Name="com.example.demo""#) && text.contains(r#"Version="1.2.3.0""#),
            "{text}"
        );
        assert!(
            text.contains(r#"Uri="https://example.com/demo-1.2.3.msix""#)
                && text.contains("OnLaunch"),
            "{text}"
        );
    }

    #[test]
    fn the_manifest_carries_the_identity_the_application_itself_uses() {
        let manifest = manifest(&config(&[]));
        assert!(manifest.contains(r#"Name="com.example.demo""#), "{manifest}");
        assert!(manifest.contains(r#"Publisher="CN=Example Ltd, O=Example""#), "{manifest}");
        assert!(manifest.contains(r#"Version="1.2.3.0""#), "{manifest}");
        assert!(manifest.contains("<PublisherDisplayName>Example Ltd<"), "{manifest}");
        assert!(manifest.contains(r#"Executable="demo.exe""#), "{manifest}");
        assert!(manifest.contains("Windows.FullTrustApplication"), "{manifest}");
        assert!(manifest.contains("runFullTrust"), "{manifest}");
        assert!(manifest.contains("Demo &amp; Co"), "XML is escaped: {manifest}");
        assert!(!manifest.contains("<Extensions>"), "no protocols, no extensions element");
    }

    #[test]
    fn every_url_scheme_becomes_a_protocol_extension() {
        let manifest = manifest(&config(&["demo", "demo-beta"]));
        assert!(manifest.contains(r#"<uap:Protocol Name="demo" />"#), "{manifest}");
        assert!(manifest.contains(r#"<uap:Protocol Name="demo-beta" />"#), "{manifest}");
        assert_eq!(manifest.matches("<Extensions>").count(), 1);
    }

    #[test]
    fn a_package_version_always_ends_in_zero() {
        assert_eq!(four_part("1.2.3"), "1.2.3.0");
        assert_eq!(four_part("2"), "2.0.0.0");
    }

    #[test]
    fn the_placeholder_is_a_png_with_correct_chunk_checksums() {
        let png = placeholder_png();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // Walk the chunks, checking each CRC the way a decoder would.
        let mut at = 8;
        let mut kinds = Vec::new();
        while at + 12 <= png.len() {
            let length =
                u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
            let kind = &png[at + 4..at + 8];
            let body = &png[at + 4..at + 8 + length];
            let stored = u32::from_be_bytes([
                png[at + 8 + length],
                png[at + 9 + length],
                png[at + 10 + length],
                png[at + 11 + length],
            ]);
            assert_eq!(stored, crc32fast::hash(body), "chunk {:?}", std::str::from_utf8(kind));
            kinds.push(String::from_utf8_lossy(kind).into_owned());
            at += 12 + length;
        }
        assert_eq!(kinds, vec!["IHDR", "IDAT", "IEND"]);
        assert_eq!(at, png.len(), "no trailing bytes");
    }
}
