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
// The host's backend: GTK on Linux, Win32 elsewhere.
#[cfg(target_os = "linux")]
use rustnative_linux::{LinuxPlatform as HostPlatform, run_catalogue};
#[cfg(not(target_os = "linux"))]
use rustnative_windows::{WindowsPlatform as HostPlatform, run_catalogue};
use {{crate_name}}::{APP_ID, APP_NAME, App};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(first) = rustnative_core::preview::requested() {
        run_catalogue({{crate_name}}::previews(), &first)?;
        return Ok(());
    }
    let mut application = Application::new(
        App::new(()),
        Window::new(APP_NAME, Size::new(480, 320)),
    );
    // The theme `app.css` describes, compiled by the build script.
    application.set_theme(rustnative_core::app_theme!());
    HostPlatform::new().with_app_id(APP_ID).run(&mut application)?;
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

/// The web template's `src/lib.rs` (`rustnative new --web`): a page with an
/// interactive island, served by the application and exportable as static
/// files.
pub const WEB_LIB_RS: &str = r#"//! {{display_name}}: its pages, its interactive parts, and the application
//! that serves them.
//!
//! - `counter` is a client component: it runs in the browser (compiled to
//!   JavaScript) and its first view is rendered on the server.
//! - [`app`] serves the pages; [`site`] is the same page as static files.

use rustnative_core::{Component, ComponentContext, Event, Node};
use rustnative_server::{ServerApp, get};
use rustnative_web::{Client, Head, Page};

#[rustnative_web::client]
pub mod counter {
    use rustnative_core::{Event, Node, NodeId};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    /// A counter: the page's interactive part.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Counter {
        /// How many times the button was pressed.
        pub clicks: u32,
    }

    impl Counter {
        /// Counts presses.
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = fx;
            if let Event::Click { target } = event {
                if target == NodeId::from_key("click") {
                    self.clicks += 1;
                }
            }
        }

        /// The count and the button.
        #[must_use]
        pub fn view(&self) -> Node {
            Node::column(
                "counter",
                [
                    Node::label("count", format!("Clicked {} times", self.clicks)),
                    Node::button("click", "Click me"),
                ],
            )
        }
    }
}

/// The home page: a greeting, and the counter.
pub struct Home;

impl Component for Home {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("home", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let counter = context.child_with_props::<Client<counter::Counter>, _>(
            "counter",
            counter::Counter::default(),
            Client::new,
        );
        Node::column("home", [Node::label("greeting", "Hello from {{display_name}}"), counter])
    }
}

/// The home page.
#[must_use]
pub fn home() -> Page {
    Page::new::<Home>(Head::new("{{display_name}}", "{{display_name}}, on the Web."), ()).lang("en")
}

/// The application: its routes and the client components its pages use.
#[must_use]
pub fn app() -> ServerApp {
    ServerApp::new().client::<counter::Counter>().route("/", get(|| async { home() }).public())
}

/// The same pages as static files (`rustnative build web --mode client`).
#[must_use]
pub fn site() -> rustnative_web::export::Site {
    rustnative_web::export::Site::new().page("/", home)
}
"#;

/// The web template's `src/main.rs`: serves the application, or writes
/// the static site when `rustnative build web --mode client` asks.
pub const WEB_MAIN_RS: &str = r#"//! Serves {{display_name}} (`rustnative run web`, `rustnative dev web`), or
//! writes it as static files (`rustnative build web --mode client`).

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if rustnative_web::export::export_folder().is_some() {
        return rustnative_web::export::run(&{{crate_name}}::site());
    }
    let address = rustnative_server::web::address("127.0.0.1:3000");
    let listener = match tokio::net::TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("could not listen on {address}: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("{{name}}: http://{address}");
    match {{crate_name}}::app().into_service().serve(listener, std::future::pending()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
"#;

/// The web template's test: the page renders on the server, with its
/// island marked for the browser.
pub const WEB_TEST_RS: &str = r#"//! The home page, rendered as the server renders it.

#[tokio::test]
async fn the_home_page_greets_and_counts() {
    let service = {{crate_name}}::app().into_service();
    let request = http::Request::get("/").body(bytes::Bytes::new()).unwrap();
    let response = service.handle(request, None).await;
    assert_eq!(response.status(), 200);
    let html = String::from_utf8(response.body().to_vec()).unwrap();
    assert!(html.contains("Hello from {{display_name}}"), "{html}");
    assert!(html.contains("data-rn-i=\"0\""), "the counter is an island: {html}");
}
"#;

/// The web template's `Cargo.toml`.
pub const WEB_CARGO_TOML: &str = r#"[package]
name = "{{name}}"
version = "{{version}}"
edition = "2024"
rust-version = "1.85"
publish = false

[dependencies]
{{dependencies}}serde = { version = "1", features = ["derive"] }
tokio = { version = "1.53", features = ["rt-multi-thread", "macros", "net"] }

[dev-dependencies]
http = "1"
bytes = "1"
"#;

/// The web template's `README.md`.
pub const WEB_README: &str = r"# {{display_name}}

A [Rust Native](https://github.com/<org>/RustNative) web application.

```sh
rustnative dev web                        # serve it, rebuild and reload on save
rustnative run web                        # serve it
rustnative build web --mode client        # static files in target/web/client
rustnative serve static target/web/client # serve them as a static host does
rustnative build web --mode serverless --host lambda  # a function
rustnative package web                    # the server as one archive
cargo test                                # the page, rendered as the server renders it
```

`rustnative.toml` holds the application's identity and, under `[web]`, the
address `run` and `dev` serve on.
";
