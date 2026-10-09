//! The Android project an application is packaged through (`PLAN.md`
//! Milestone 35: "packaging: Gradle with the SDK and NDK, a manifest
//! generated from `rustnative.toml`, signing, and APK/AAB output").
//!
//! [`AndroidProject::files`] writes everything Gradle needs except the
//! compiled libraries and the backend's Java host library, which
//! `rustnative package android` copies in: the settings and build scripts,
//! the manifest (activities, deep-link and share filters, the widget
//! receiver and tile service, the permissions the application declares),
//! the theme, and the strings. The files are build outputs, regenerated on
//! every build and never edited by hand (`C63`); the same input writes the
//! same bytes.

use std::fmt::Write as _;

/// A home-screen widget the application declares (`[[android.widgets]]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidWidget {
    /// Its id (`SurfaceCommand::UpdateWidget`'s `id`).
    pub id: String,
    /// What the widget picker calls it.
    pub label: String,
}

/// A quick-settings tile the application declares (`[[android.tiles]]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidTile {
    /// Its id (`SurfaceCommand::UpdateTile`'s `id`).
    pub id: String,
    /// What the tile shows before the application updates it.
    pub label: String,
}

/// An asset pack in the application bundle (`[[android.asset-packs]]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidAssetPack {
    /// Its module name.
    pub name: String,
    /// `install-time`, `fast-follow`, or `on-demand`.
    pub delivery: String,
}

/// Release signing: where the keystore is, and which environment variables
/// hold its passwords — never the passwords themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidSigning {
    /// The keystore file (absolute, or relative to the generated project).
    pub store: String,
    /// The key's alias.
    pub alias: String,
    /// The environment variable holding the store's password.
    pub store_password_env: String,
    /// The environment variable holding the key's password.
    pub key_password_env: String,
}

/// How many widgets, and how many tiles, an application may declare: each
/// needs a component of its own, and the host library has this many
/// (`RnWidgetProvider.Slot0`…, `RnTileService.Slot0`…).
pub const SURFACE_SLOTS: usize = 4;

/// Everything the generated project depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidProject {
    /// The application id (`com.example.notes`).
    pub application_id: String,
    /// The name shown to people.
    pub label: String,
    /// The version people see.
    pub version_name: String,
    /// The version Android compares.
    pub version_code: u32,
    /// The Rust library's name: `lib<name>.so` in `jniLibs`.
    pub library: String,
    /// The oldest API level it runs on.
    pub min_sdk: u32,
    /// The API level it is built against and targets.
    pub target_sdk: u32,
    /// URL schemes it opens (`notes` for `notes://…`).
    pub url_schemes: Vec<String>,
    /// MIME types it accepts shares of (empty: not a share target).
    pub share_types: Vec<String>,
    /// Permissions it uses (`android.permission.CAMERA`).
    pub permissions: Vec<String>,
    /// Home-screen widgets.
    pub widgets: Vec<AndroidWidget>,
    /// Quick-settings tiles.
    pub tiles: Vec<AndroidTile>,
    /// Asset packs (in an application bundle).
    pub asset_packs: Vec<AndroidAssetPack>,
    /// Release signing, when the application has a keystore.
    pub signing: Option<AndroidSigning>,
    /// Whether the project carries the device suite's instrumentation.
    pub instrumentation: bool,
    /// Whether an icon is copied into `res/mipmap-*` (`ic_launcher`).
    pub icon: bool,
    /// The Android Gradle Plugin version.
    pub gradle_plugin: String,
    /// The application's own launcher activity (a host application that
    /// embeds `RustNativeView`), in place of `RnActivity`.
    pub launcher: Option<String>,
    /// The NDK the libraries were built with (`ndkVersion`), so the
    /// Android Gradle Plugin strips their symbols with it.
    pub ndk_version: Option<String>,
}

/// The configuration changes the activity handles itself — every one,
/// so the application outlives rotation, night mode, font scale, locale,
/// and resizing in place (`lifecycle.rs`).
pub const CONFIG_CHANGES: &str = "orientation|screenSize|smallestScreenSize|screenLayout|keyboard|keyboardHidden|navigation|uiMode|fontScale|locale|layoutDirection|density|touchscreen|colorMode|mcc|mnc|fontWeightAdjustment";

impl AndroidProject {
    /// A project for `application_id` with the defaults the backend
    /// supports: API 26 to 35, no extras.
    #[must_use]
    pub fn new(application_id: &str, label: &str, version: &str, library: &str) -> Self {
        Self {
            application_id: application_id_from(application_id),
            label: label.to_owned(),
            version_name: version.to_owned(),
            version_code: version_code(version),
            library: library.replace('-', "_"),
            min_sdk: 26,
            target_sdk: 35,
            url_schemes: Vec::new(),
            share_types: Vec::new(),
            permissions: Vec::new(),
            widgets: Vec::new(),
            tiles: Vec::new(),
            asset_packs: Vec::new(),
            signing: None,
            instrumentation: false,
            icon: false,
            gradle_plugin: "8.7.3".to_owned(),
            launcher: None,
            ndk_version: None,
        }
    }

    /// Every generated file, as a path relative to the project and its
    /// contents.
    #[must_use]
    pub fn files(&self) -> Vec<(String, String)> {
        let mut files = vec![
            ("settings.gradle".to_owned(), self.settings()),
            ("build.gradle".to_owned(), format!("plugins {{\n    id 'com.android.application' version '{}' apply false\n    id 'com.android.asset-pack' version '{}' apply false\n}}\n", self.gradle_plugin, self.gradle_plugin)),
            ("gradle.properties".to_owned(), "org.gradle.jvmargs=-Xmx2g -Dfile.encoding=UTF-8\nandroid.useAndroidX=false\nandroid.nonTransitiveRClass=true\n".to_owned()),
            ("app/build.gradle".to_owned(), self.app_build()),
            ("app/src/main/AndroidManifest.xml".to_owned(), self.manifest()),
            ("app/src/main/res/values/strings.xml".to_owned(), self.strings()),
            ("app/src/main/res/values/styles.xml".to_owned(), theme("@android:style/Theme.DeviceDefault.Light.NoActionBar")),
            ("app/src/main/res/values-night/styles.xml".to_owned(), theme("@android:style/Theme.DeviceDefault.NoActionBar")),
        ];
        if !self.widgets.is_empty() {
            files.push((
                "app/src/main/res/layout/rn_widget.xml".to_owned(),
                WIDGET_LAYOUT.to_owned(),
            ));
            for widget in &self.widgets {
                files.push((
                    format!("app/src/main/res/xml/rn_widget_{}.xml", resource_name(&widget.id)),
                    widget_info(widget),
                ));
            }
        }
        for pack in &self.asset_packs {
            files.push((format!("{}/build.gradle", pack.name), format!("plugins {{\n    id 'com.android.asset-pack'\n}}\n\nassetPack {{\n    packName = \"{}\"\n    dynamicDelivery {{\n        deliveryType = \"{}\"\n    }}\n}}\n", pack.name, pack.delivery)));
        }
        files
    }

    fn settings(&self) -> String {
        let mut out = String::from(
            "pluginManagement {\n    repositories {\n        google()\n        mavenCentral()\n        gradlePluginPortal()\n    }\n}\ndependencyResolutionManagement {\n    repositories {\n        google()\n        mavenCentral()\n    }\n}\n",
        );
        let _ = writeln!(out, "rootProject.name = \"{}\"", xml(&self.label).replace('"', ""));
        out.push_str("include ':app'\n");
        for pack in &self.asset_packs {
            let _ = writeln!(out, "include ':{}'", pack.name);
        }
        out
    }

    fn app_build(&self) -> String {
        let mut out = String::from("plugins {\n    id 'com.android.application'\n}\n\nandroid {\n");
        let _ = writeln!(out, "    namespace '{}'", self.application_id);
        let _ = writeln!(out, "    compileSdk {}", self.target_sdk);
        if let Some(ndk) = &self.ndk_version {
            let _ = writeln!(out, "    ndkVersion '{}'", ndk.replace('\'', ""));
        }
        out.push_str("    defaultConfig {\n");
        let _ = writeln!(out, "        applicationId '{}'", self.application_id);
        let _ = writeln!(out, "        minSdk {}", self.min_sdk);
        let _ = writeln!(out, "        targetSdk {}", self.target_sdk);
        let _ = writeln!(out, "        versionCode {}", self.version_code);
        let _ = writeln!(out, "        versionName '{}'", self.version_name.replace('\'', ""));
        out.push_str("    }\n");
        out.push_str("    compileOptions {\n        sourceCompatibility JavaVersion.VERSION_17\n        targetCompatibility JavaVersion.VERSION_17\n    }\n");
        if let Some(signing) = &self.signing {
            out.push_str("    signingConfigs {\n        release {\n");
            let _ =
                writeln!(out, "            storeFile file('{}')", signing.store.replace('\\', "/"));
            let _ = writeln!(
                out,
                "            storePassword System.getenv('{}')",
                signing.store_password_env
            );
            let _ = writeln!(out, "            keyAlias '{}'", signing.alias);
            let _ = writeln!(
                out,
                "            keyPassword System.getenv('{}')",
                signing.key_password_env
            );
            out.push_str("        }\n    }\n");
        }
        out.push_str("    buildTypes {\n        release {\n            minifyEnabled false\n");
        if self.signing.is_some() {
            out.push_str("            signingConfig signingConfigs.release\n");
        }
        out.push_str("        }\n    }\n");
        out.push_str("    packagingOptions {\n        jniLibs {\n            useLegacyPackaging = false\n        }\n    }\n");
        if !self.asset_packs.is_empty() {
            let packs: Vec<String> =
                self.asset_packs.iter().map(|pack| format!("':{}'", pack.name)).collect();
            let _ = writeln!(out, "    assetPacks = [{}]", packs.join(", "));
        }
        out.push_str("}\n");
        out
    }

    fn strings(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n");
        let _ = writeln!(out, "    <string name=\"app_name\">{}</string>", xml(&self.label));
        for widget in &self.widgets {
            let _ = writeln!(
                out,
                "    <string name=\"rn_widget_{}\">{}</string>",
                resource_name(&widget.id),
                xml(&widget.label)
            );
        }
        out.push_str("</resources>\n");
        out
    }

    /// The manifest.
    #[must_use]
    pub fn manifest(&self) -> String {
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">\n",
        );
        let mut permissions = self.permissions.clone();
        // Normal permissions the host library always needs: constrained jobs
        // and the network condition read the connectivity state.
        permissions.push("android.permission.ACCESS_NETWORK_STATE".to_owned());
        if !self.tiles.is_empty() || !self.widgets.is_empty() {
            permissions.push("android.permission.POST_NOTIFICATIONS".to_owned());
        }
        permissions.sort();
        permissions.dedup();
        for permission in &permissions {
            let _ = writeln!(out, "    <uses-permission android:name=\"{}\" />", xml(permission));
        }
        if self.instrumentation {
            let _ = writeln!(
                out,
                "    <instrumentation android:name=\"dev.rustnative.android.RnInstrumentation\" android:targetPackage=\"{}\" android:label=\"Rust Native device suite\" />",
                self.application_id
            );
        }
        out.push_str("    <application\n        android:label=\"@string/app_name\"\n");
        if self.icon {
            out.push_str("        android:icon=\"@mipmap/ic_launcher\"\n");
        }
        out.push_str("        android:theme=\"@style/RnTheme\"\n        android:enableOnBackInvokedCallback=\"true\"\n        android:supportsRtl=\"true\"\n        android:allowBackup=\"true\">\n");
        let _ = writeln!(
            out,
            "        <meta-data android:name=\"dev.rustnative.library\" android:value=\"{}\" />",
            xml(&self.library)
        );
        out.push_str("        <uses-library android:name=\"androidx.window.extensions\" android:required=\"false\" />\n");
        // The launcher activity: window 0, the primary window — or the
        // application's own, which embeds `RustNativeView`.
        if let Some(launcher) = &self.launcher {
            let _ = writeln!(
                out,
                "        <activity\n            android:name=\"{}\"\n            android:exported=\"true\"\n            android:configChanges=\"{CONFIG_CHANGES}\">\n            <intent-filter>\n                <action android:name=\"android.intent.action.MAIN\" />\n                <category android:name=\"android.intent.category.LAUNCHER\" />\n            </intent-filter>\n        </activity>",
                xml(launcher)
            );
        }
        out.push_str("        <activity\n            android:name=\"dev.rustnative.android.RnActivity\"\n            android:exported=\"true\"\n            android:launchMode=\"singleTask\"\n");
        let _ = writeln!(out, "            android:configChanges=\"{CONFIG_CHANGES}\"");
        out.push_str("            android:windowSoftInputMode=\"adjustResize\">\n");
        if self.launcher.is_none() {
            out.push_str("            <intent-filter>\n                <action android:name=\"android.intent.action.MAIN\" />\n                <category android:name=\"android.intent.category.LAUNCHER\" />\n            </intent-filter>\n");
        }
        for scheme in &self.url_schemes {
            out.push_str("            <intent-filter>\n                <action android:name=\"android.intent.action.VIEW\" />\n                <category android:name=\"android.intent.category.DEFAULT\" />\n                <category android:name=\"android.intent.category.BROWSABLE\" />\n");
            let _ = writeln!(out, "                <data android:scheme=\"{}\" />", xml(scheme));
            out.push_str("            </intent-filter>\n");
        }
        if !self.share_types.is_empty() {
            for action in ["android.intent.action.SEND", "android.intent.action.SEND_MULTIPLE"] {
                out.push_str("            <intent-filter>\n");
                let _ = writeln!(out, "                <action android:name=\"{action}\" />");
                out.push_str("                <category android:name=\"android.intent.category.DEFAULT\" />\n");
                for mime in &self.share_types {
                    let _ = writeln!(
                        out,
                        "                <data android:mimeType=\"{}\" />",
                        xml(mime)
                    );
                }
                out.push_str("            </intent-filter>\n");
            }
        }
        out.push_str("        </activity>\n");
        // Every other window: another instance of the same activity, in a
        // task of its own.
        out.push_str("        <activity\n            android:name=\"dev.rustnative.android.RnWindowActivity\"\n            android:exported=\"false\"\n            android:documentLaunchMode=\"intoExisting\"\n");
        let _ = writeln!(out, "            android:configChanges=\"{CONFIG_CHANGES}\"");
        out.push_str("            android:windowSoftInputMode=\"adjustResize\" />\n");
        if self.instrumentation {
            out.push_str("        <activity\n            android:name=\"dev.rustnative.android.RnTestActivity\"\n            android:exported=\"false\"\n");
            let _ = writeln!(out, "            android:configChanges=\"{CONFIG_CHANGES}\"");
            out.push_str("            android:windowSoftInputMode=\"adjustResize\" />\n");
        }
        // Background work (`JobScheduler`), always present: the data
        // layer's constrained jobs run here.
        out.push_str("        <service\n            android:name=\"dev.rustnative.android.RnJobService\"\n            android:permission=\"android.permission.BIND_JOB_SERVICE\"\n            android:exported=\"false\" />\n");
        for (index, widget) in self.widgets.iter().enumerate() {
            let name = resource_name(&widget.id);
            let _ = writeln!(
                out,
                "        <receiver\n            android:name=\"dev.rustnative.android.RnWidgetProvider$Slot{index}\"\n            android:label=\"@string/rn_widget_{name}\"\n            android:exported=\"false\">",
            );
            out.push_str("            <intent-filter>\n                <action android:name=\"android.appwidget.action.APPWIDGET_UPDATE\" />\n            </intent-filter>\n");
            let _ = writeln!(
                out,
                "            <meta-data android:name=\"android.appwidget.provider\" android:resource=\"@xml/rn_widget_{name}\" />"
            );
            let _ = writeln!(
                out,
                "            <meta-data android:name=\"dev.rustnative.surface\" android:value=\"{}\" />",
                xml(&widget.id)
            );
            out.push_str("        </receiver>\n");
        }
        for (index, tile) in self.tiles.iter().enumerate() {
            let _ = writeln!(
                out,
                "        <service\n            android:name=\"dev.rustnative.android.RnTileService$Slot{index}\"\n            android:label=\"{}\"\n            android:permission=\"android.permission.BIND_QUICK_SETTINGS_TILE\"\n            android:exported=\"true\">",
                xml(&tile.label)
            );
            out.push_str("            <intent-filter>\n                <action android:name=\"android.service.quicksettings.action.QS_TILE\" />\n            </intent-filter>\n");
            let _ = writeln!(
                out,
                "            <meta-data android:name=\"dev.rustnative.surface\" android:value=\"{}\" />",
                xml(&tile.id)
            );
            out.push_str("        </service>\n");
        }
        out.push_str("    </application>\n</manifest>\n");
        out
    }
}

fn theme(parent: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n    <style name=\"RnTheme\" parent=\"{parent}\">\n        <item name=\"android:windowLightStatusBar\">?android:attr/isLightTheme</item>\n        <item name=\"android:statusBarColor\">@android:color/transparent</item>\n        <item name=\"android:navigationBarColor\">@android:color/transparent</item>\n    </style>\n</resources>\n"
    )
}

const WIDGET_LAYOUT: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<LinearLayout xmlns:android=\"http://schemas.android.com/apk/res/android\"\n    android:id=\"@android:id/background\"\n    android:layout_width=\"match_parent\"\n    android:layout_height=\"match_parent\"\n    android:orientation=\"vertical\"\n    android:padding=\"12dp\"\n    android:background=\"@android:drawable/dialog_holo_light_frame\" />\n";

fn widget_info(widget: &AndroidWidget) -> String {
    let _ = widget;
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<appwidget-provider xmlns:android=\"http://schemas.android.com/apk/res/android\"\n    android:minWidth=\"180dp\"\n    android:minHeight=\"110dp\"\n    android:targetCellWidth=\"3\"\n    android:targetCellHeight=\"2\"\n    android:resizeMode=\"horizontal|vertical\"\n    android:updatePeriodMillis=\"0\"\n    android:initialLayout=\"@layout/rn_widget\"\n    android:widgetCategory=\"home_screen\" />\n".to_owned()
}

/// An application id Android accepts from `rustnative.toml`'s `app.id`:
/// segments of letters, digits, and underscores, each starting with a
/// letter (`dev.rustnative.hello-label` → `dev.rustnative.hello_label`).
#[must_use]
pub fn application_id_from(id: &str) -> String {
    let segments: Vec<String> = id
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let cleaned: String = segment
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
                .collect();
            if cleaned.starts_with(|c: char| c.is_ascii_alphabetic()) {
                cleaned
            } else {
                format!("a{cleaned}")
            }
        })
        .collect();
    match segments.len() {
        0 => "dev.rustnative.app".to_owned(),
        1 => format!("dev.rustnative.{}", segments[0]),
        _ => segments.join("."),
    }
}

/// The version code for `major.minor.patch`: `major·10000 + minor·100 +
/// patch`, at least 1 (Android refuses 0).
#[must_use]
pub fn version_code(version: &str) -> u32 {
    let mut parts = version.split(['.', '-', '+']).map(|part| part.parse::<u32>().unwrap_or(0));
    let (major, minor, patch) =
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    major
        .saturating_mul(10_000)
        .saturating_add(minor.min(99) * 100)
        .saturating_add(patch.min(99))
        .max(1)
}

fn resource_name(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect()
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_versions_become_what_android_accepts() {
        assert_eq!(application_id_from("dev.rustnative.hello-label"), "dev.rustnative.hello_label");
        assert_eq!(application_id_from("com.example.2do"), "com.example.a2do");
        assert_eq!(application_id_from("notes"), "dev.rustnative.notes");
        assert_eq!(version_code("1.2.3"), 10_203);
        assert_eq!(version_code("0.1.0"), 100);
        assert_eq!(version_code("0.0.0"), 1);
    }

    #[test]
    fn the_manifest_declares_what_the_project_says() {
        let mut project =
            AndroidProject::new("dev.rustnative.notes", "Notes & More", "1.0.0", "notes-app");
        project.url_schemes = vec!["notes".into()];
        project.share_types = vec!["text/plain".into()];
        project.permissions = vec!["android.permission.CAMERA".into()];
        project.widgets = vec![AndroidWidget { id: "summary".into(), label: "Summary".into() }];
        project.tiles = vec![AndroidTile { id: "quick".into(), label: "Quick note".into() }];
        project.instrumentation = true;
        let manifest = project.manifest();
        assert!(manifest.contains("android:value=\"notes_app\""));
        assert!(manifest.contains("<data android:scheme=\"notes\" />"));
        assert!(manifest.contains("android.intent.action.SEND_MULTIPLE"));
        assert!(manifest.contains("android.permission.CAMERA"));
        assert!(manifest.contains("android.permission.POST_NOTIFICATIONS"));
        assert!(manifest.contains("android.permission.ACCESS_NETWORK_STATE"));
        assert!(manifest.contains("RnWidgetProvider$Slot0"));
        assert!(manifest.contains("RnTileService$Slot0"));
        assert!(manifest.contains("RnInstrumentation"));
        assert!(manifest.contains(CONFIG_CHANGES));
        assert!(
            manifest.contains("android:supportsRtl=\"true\""),
            "right to left is the framework's to decide"
        );
        let files = project.files();
        let strings = &files
            .iter()
            .find(|(path, _)| path.ends_with("values/strings.xml"))
            .expect("strings")
            .1;
        assert!(strings.contains("Notes &amp; More"));
        // The same input writes the same bytes.
        assert_eq!(files, project.files());
    }

    #[test]
    fn signing_names_environment_variables_never_passwords() {
        let mut project = AndroidProject::new("dev.rustnative.notes", "Notes", "1.0.0", "notes");
        project.signing = Some(AndroidSigning {
            store: "C:\\keys\\release.jks".into(),
            alias: "release".into(),
            store_password_env: "NOTES_STORE_PASSWORD".into(),
            key_password_env: "NOTES_KEY_PASSWORD".into(),
        });
        let build = project.app_build();
        assert!(build.contains("storeFile file('C:/keys/release.jks')"));
        assert!(build.contains("System.getenv('NOTES_STORE_PASSWORD')"));
        assert!(build.contains("signingConfig signingConfigs.release"));
    }
}
