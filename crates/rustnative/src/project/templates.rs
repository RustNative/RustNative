//! What a new project is made of.
//!
//! Small on purpose: a window, a button, and the three files that make it a
//! project (`Cargo.toml`, `rustnative.toml`, `.gitignore`). Everything a person is
//! likely to change is in `src/main.rs`, and everything the tooling reads
//! is in `rustnative.toml`.

/// The generated `src/lib.rs`: the application itself — its root
/// component and its previews. The executable (`src/main.rs`) is a thin
/// shell around it, so a change to the application recompiles this crate
/// and relinks the shell (`PLAN.md` Milestone 43).
pub const LIB_RS: &str = r#"//! {{display_name}}: its root component and its previews.

use rustnative_core::preview::{Preview, PreviewMatrix};
use rustnative_core::{Component, Event, Node, NodeId, classes};

/// The application's name, as people see it.
pub const APP_NAME: &str = "{{display_name}}";
/// The application's identity: its saved state and single instance.
pub const APP_ID: &str = "{{app_id}}";

/// The application's root component: state, a view of it, and what events
/// do to it.
pub struct App {
    clicks: u32,
}

impl Component for App {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self { clicks: 0 }
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::label("greeting", format!("Hello from {APP_NAME}"))
                    .with_class(classes!("headline")),
                Node::label("count", format!("Clicked {} times", self.clicks)),
                Node::button("click", "Click me"),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("click")) {
            self.clicks += 1;
        }
    }
}

/// The application's previews: `rustnative preview` browses them, and
/// `tests/previews.rs` makes each one a golden test.
#[must_use]
pub fn previews() -> Vec<Preview> {
    vec![Preview::component::<App>("app", ()).with_matrix(PreviewMatrix::full())]
}
"#;

/// The generated `src/main.rs`: the shell that runs the application — or,
/// under `rustnative preview`, its preview catalogue.
pub const MAIN_RS: &str = r#"#![cfg_attr(windows, windows_subsystem = "windows")]

use rustnative_core::{Application, Component, Platform, Size, Window};
use rustnative_windows::WindowsPlatform;
use {{crate_name}}::{APP_ID, APP_NAME, App};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(first) = rustnative_core::preview::requested() {
        rustnative_windows::run_catalogue({{crate_name}}::previews(), &first)?;
        return Ok(());
    }
    let mut application = Application::new(
        App::new(()),
        Window::new(APP_NAME, Size::new(480, 320)),
    );
    // The theme `app.css` describes, compiled by the build script.
    application.set_theme(rustnative_core::app_theme!());
    WindowsPlatform::new().with_app_id(APP_ID).run(&mut application)?;
    Ok(())
}
"#;

/// The generated `tests/previews.rs`: every preview, in every
/// configuration, is a golden test (`C55-3`).
pub const PREVIEWS_TEST_RS: &str = r#"//! Every preview is a golden test: a change to what one shows fails here
//! until it is reviewed and blessed (`RUSTNATIVE_BLESS=1 rustnative test`).

#[test]
fn every_preview_matches_its_golden() {
    let goldens = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens");
    rustnative_headless::preview_goldens(&{{crate_name}}::previews(), &goldens);
}
"#;

/// The generated `Cargo.toml`, with `{{dependencies}}` and
/// `{{build-dependencies}}` filled in depending on whether the project is
/// built against a checkout of the framework or against published versions.
pub const CARGO_TOML: &str = r#"[package]
name = "{{name}}"
version = "{{version}}"
edition = "2024"
rust-version = "1.85"
publish = false

[dependencies]
{{dependencies}}
[dev-dependencies]
{{dev-dependencies}}
[build-dependencies]
{{build-dependencies}}
"#;

/// The generated `build.rs`, which compiles the style file and gives the
/// executable its icon, version information, and application manifest.
pub const BUILD_RS: &str = r"//! Compiles `app.css` into this application's theme, and embeds its icon,
//! version information, and Windows application manifest
//! (`rustnative.toml`).

fn main() {
    rustnative_build::compile_styles();
    rustnative_build::embed_resources();
}
";

/// The generated `app.css`: the project's style file (`PLAN.md` 2.14).
pub const APP_CSS: &str = r"/* The application's style file: theme tokens over the default theme
   (Tailwind CSS v4's), and the project's own utilities. Classes are
   checked when the application compiles; an unknown one is an error. */

@theme {
  --color-accent: oklch(0.55 0.19 255);
}

@utility headline {
  @apply text-lg font-semibold text-accent;
}
";

/// The generated `.gitignore`.
pub const GITIGNORE: &str = "/target\n";

/// The generated `README.md`.
pub const README: &str = r"# {{display_name}}

A [Rust Native](https://github.com/<org>/RustNative) application.

```sh
rustnative run windows      # build and run
rustnative build windows    # build only, `--release` for an optimized build
rustnative test             # run the project's tests (every preview is a golden test)
rustnative preview          # browse the previews across themes, locales, text sizes
rustnative dev windows      # rebuild and restart on save, keeping the application's state
rustnative doctor           # check the toolchains this machine has
```

`rustnative.toml` holds what the tooling needs to know about this application: its
identity (used for its saved state and to keep one instance running), the
name people see, and its version.
";

/// The markup template's `src/lib.rs`: the same application as
/// [`LIB_RS`], with its component in `src/app.rsx`.
pub const MARKUP_LIB_RS: &str = r#"//! {{display_name}}: its root component and its previews.

use rustnative_core::preview::{Preview, PreviewMatrix};

/// The application's name, as people see it.
pub const APP_NAME: &str = "{{display_name}}";
/// The application's identity: its saved state and single instance.
pub const APP_ID: &str = "{{app_id}}";

// The root component is written in markup: see `src/app.rsx`, which the
// build script lowers with `rustnative_build::compile_rsx()`.
rustnative_core::rsx_mod!(app);

pub use app::App;

/// The application's previews: `rustnative preview` browses them, and
/// `tests/previews.rs` makes each one a golden test.
#[must_use]
pub fn previews() -> Vec<Preview> {
    vec![Preview::component::<App>("app", ()).with_matrix(PreviewMatrix::full())]
}
"#;

/// The markup template's `src/main.rs`: the same shell as [`MAIN_RS`].
pub const MARKUP_MAIN_RS: &str = MAIN_RS;

/// The markup template's `src/app.rsx`.
pub const MARKUP_APP_RSX: &str = r#"// The application's root component: state, a view of it written in
// markup, and what events do to it.

use rustnative_core::{Component, Event, Node, NodeId};

pub struct App {
    clicks: u32,
}

impl Component for App {
    type Props = ();
    type Message = ();

    fn new((): Self::Props) -> Self {
        Self { clicks: 0 }
    }

    fn props(&self) -> &Self::Props {
        static PROPS: () = ();
        &PROPS
    }

    fn set_props(&mut self, (): Self::Props) {}

    fn view(&self) -> Node {
        <Column key="root">
            <Label
                key="greeting"
                text={format!("Hello from {}", super::APP_NAME)}
                class="headline"
            />
            <Label key="count" text={format!("Clicked {} times", self.clicks)} />
            <Button key="click" text="Click me" />
        </Column>
    }

    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("click")) {
            self.clicks += 1;
        }
    }
}
"#;

/// The markup template's `build.rs`: resources, and the `.rsx` lowering.
pub const MARKUP_BUILD_RS: &str = r"//! Lowers this application's `.rsx` files, compiles `app.css` into its
//! theme, and embeds its icon, version information, and Windows
//! application manifest (`rustnative.toml`).

fn main() {
    rustnative_build::compile_rsx();
    rustnative_build::compile_styles();
    rustnative_build::embed_resources();
}
";
