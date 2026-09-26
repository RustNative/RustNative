//! The application manifest every Rust Native executable is built with.
//!
//! Five things it declares, each load-bearing:
//!
//! - **Per-monitor V2 DPI awareness**, so Windows does not stretch the
//!   window's pixels when it moves to a monitor with a different scale;
//! - **Common Controls version 6**, without which the system controls this
//!   framework realizes are drawn in their Windows 95 form and the tab
//!   control from Milestone 30 is unthemed;
//! - **`supportedOS` for Windows 8.1, 10, and 11**, which is what makes
//!   `GetVersion` and — the reason it is not optional here — *layered child
//!   windows* behave as documented, the mechanism Milestone 27 animates
//!   opacity with;
//! - **UTF-8 as the active code page**, so the `*A` entry points any
//!   dependency calls do not mangle non-ASCII text;
//! - **long-path awareness**, so a file dialog can return a path longer
//!   than 260 characters and the application can open it.

/// The manifest's XML, with `{{name}}` filled in.
const TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="{{name}}" version="{{version}}" processorArchitecture="*" />
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*" />
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 8.1, 10, and 11: without these a process is lied to about
           the Windows version, and layered child windows do not work. -->
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}" />
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

/// The manifest for an application called `name` at `version`
/// (`major.minor.patch`).
#[must_use]
pub fn application_manifest(name: &str, version: &str) -> String {
    TEMPLATE.replace("{{name}}", &escape(name)).replace("{{version}}", &four_part_version(version))
}

/// A manifest version is always four numbers; `rustnative.toml` carries three.
fn four_part_version(version: &str) -> String {
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(4)
        .map(|part| part.to_string())
        .collect::<Vec<_>>();
    parts.truncate(4);
    parts.join(".")
}

/// XML-escapes the characters that would otherwise end an attribute.
pub(crate) fn escape(text: &str) -> String {
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
    fn the_manifest_declares_everything_this_framework_depends_on() {
        let manifest = application_manifest("demo", "1.2.3");
        for required in [
            ">PerMonitorV2<",
            "Microsoft.Windows.Common-Controls",
            "6.0.0.0",
            ">UTF-8<",
            ">true<",
            "{1f676c76-80e1-4239-95bb-83d0f6d0da78}",
            "{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}",
            "asInvoker",
        ] {
            assert!(manifest.contains(required), "the manifest declares {required}:\n{manifest}");
        }
        assert!(manifest.contains("name=\"demo\" version=\"1.2.3.0\""), "{manifest}");
    }

    #[test]
    fn a_name_with_xml_in_it_cannot_break_the_manifest() {
        let manifest = application_manifest("A & B\"</assembly>", "1.0.0");
        assert!(manifest.contains("A &amp; B&quot;&lt;/assembly&gt;"), "{manifest}");
        assert_eq!(manifest.matches("</assembly>").count(), 1, "still one document");
    }

    #[test]
    fn versions_are_padded_and_trimmed_to_four_parts() {
        assert_eq!(four_part_version("1.2.3"), "1.2.3.0");
        assert_eq!(four_part_version("1"), "1.0.0.0");
        assert_eq!(four_part_version("1.2.3.4.5"), "1.2.3.4");
        assert_eq!(four_part_version("x.y.z"), "0.0.0.0", "unparsable parts become zero");
    }
}
