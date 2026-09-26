//! `rustnative generate` (`PLAN.md` Milestone 43, `C57-1`): a component, a
//! screen wired into the application's routes, or a service — each with
//! its preview and its test, in the syntax the project is written in (a
//! project with `.rsx` files gets markup; otherwise builder calls).
//!
//! What is generated is registered where the project already declares
//! things:
//! - the module in `src/lib.rs` (`pub mod x;`, or `rsx_mod!(pub x);`);
//! - its preview in `previews()`;
//! - a screen's route in `router()`, which the first screen adds.
//!
//! Nothing that exists is overwritten.

use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::error::{Error, Result};
use crate::project::Project;

/// What to generate.
#[derive(Debug, Subcommand)]
pub enum Generate {
    /// A component, with its preview and a test.
    Component {
        /// Its name, in `PascalCase`.
        name: String,
    },
    /// A screen: a component with a route, wired into `router()`.
    Screen {
        /// Its name, in `PascalCase`.
        name: String,
        /// Its route (`/settings`, `/users/:id`).
        #[arg(long)]
        route: String,
    },
    /// A service: a trait the application asks through, an in-memory
    /// implementation for previews and tests, and a test.
    Service {
        /// Its name, in `PascalCase`.
        name: String,
    },
    /// A server resource (Milestone 49's server application model).
    ServerResource {
        /// Its name, in `PascalCase`.
        name: String,
    },
    /// A feature kit: working, tested accounts, administration, or a
    /// store, on the server application model (Milestone 52).
    Kit {
        /// Which kit.
        #[arg(value_enum)]
        kit: crate::kits::Kit,
    },
}

/// `PascalCase` to `snake_case`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.extend(character.to_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

fn valid(name: &str) -> Result<()> {
    let mut characters = name.chars();
    let starts = characters.next().is_some_and(char::is_uppercase);
    if starts && characters.all(char::is_alphanumeric) {
        Ok(())
    } else {
        Err(Error::Usage(format!("`{name}` is not a PascalCase name (`Settings`, `UserList`)")))
    }
}

/// Whether the project is written in markup: it has `.rsx` files.
fn markup(root: &Path) -> bool {
    fn any_rsx(folder: &Path) -> bool {
        std::fs::read_dir(folder).into_iter().flatten().flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                any_rsx(&path)
            } else {
                path.extension().is_some_and(|extension| extension == "rsx")
            }
        })
    }
    any_rsx(&root.join("src"))
}

fn write_new(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Err(Error::Usage(format!("{} already exists", path.display())));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|cause| Error::Io { what: format!("create {}", parent.display()), cause })?;
    }
    std::fs::write(path, contents)
        .map_err(|cause| Error::Io { what: format!("write {}", path.display()), cause })?;
    println!("generate: wrote {}", path.display());
    Ok(())
}

fn component(name: &str, key: &str, markup: bool, route: Option<&str>) -> String {
    let route = route.map_or_else(String::new, |route| {
        format!("/// Where the application's router sends to this screen.\npub const ROUTE: &str = \"{route}\";\n\n")
    });
    let view = if markup {
        format!(
            "    fn view(&self) -> Node {{\n        <Column key=\"{key}\">\n            <Label key=\"{key}-title\" text=\"{name}\" />\n        </Column>\n    }}\n"
        )
    } else {
        format!(
            "    fn view(&self) -> Node {{\n        Node::column(\"{key}\", [Node::label(\"{key}-title\", \"{name}\")])\n    }}\n"
        )
    };
    format!(
        "//! The `{name}` {kind}.

use rustnative_core::{{Component, Event, Node}};

{route}/// {name}.
pub struct {name} {{
    props: (),
}}

impl Component for {name} {{
    type Props = ();
    type Message = ();

    fn new(props: Self::Props) -> Self {{
        Self {{ props }}
    }}

    fn props(&self) -> &Self::Props {{
        &self.props
    }}

    fn set_props(&mut self, props: Self::Props) {{
        self.props = props;
    }}

{view}
    fn update(&mut self, _event: Event) {{}}
}}
",
        kind = if route.is_empty() { "component" } else { "screen" },
    )
}

fn component_test(
    crate_name: &str,
    module: &str,
    name: &str,
    key: &str,
    route: Option<&str>,
) -> String {
    let route_test = route.map_or_else(String::new, |route| {
        format!(
            "
#[test]
fn the_router_sends_its_route_here() {{
    let router = {crate_name}::router().expect(\"the routes parse\");
    assert_eq!(router.resolve(\"{route}\").map(|(name, _)| name), Some(\"{module}\"));
}}
"
        )
    });
    format!(
        "//! `{name}`: it renders, on the headless backend.

use rustnative_core::{{Component, Size, Window}};
use rustnative_headless::{{HeadlessApp, Query}};
use {crate_name}::{module}::{name};

#[test]
fn it_renders_its_title() {{
    let app = HeadlessApp::launch(Window::new(\"{name}\", Size::new(480, 320)), || {name}::new(()));
    assert!(app.find(&Query::key(\"{key}-title\")).is_ok());
}}
{route_test}"
    )
}

fn service(name: &str) -> String {
    format!(
        "//! The `{name}` service: what the application asks through, so a
//! preview or a test can answer instead of the real thing.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// {name}.
pub trait {name}: Send + Sync {{
    /// The value stored under `key`.
    fn get(&self, key: &str) -> Option<String>;
    /// Stores `value` under `key`.
    fn set(&self, key: &str, value: String);
}}

/// A `{name}` kept in memory — for previews and tests.
#[derive(Debug, Default)]
pub struct InMemory{name} {{
    values: Mutex<BTreeMap<String, String>>,
}}

impl {name} for InMemory{name} {{
    fn get(&self, key: &str) -> Option<String> {{
        self.values.lock().ok()?.get(key).cloned()
    }}

    fn set(&self, key: &str, value: String) {{
        if let Ok(mut values) = self.values.lock() {{
            values.insert(key.to_owned(), value);
        }}
    }}
}}
"
    )
}

fn service_test(crate_name: &str, module: &str, name: &str) -> String {
    format!(
        "//! `{name}`: the in-memory implementation keeps what it is given.

use {crate_name}::{module}::{{InMemory{name}, {name}}};

#[test]
fn it_keeps_what_it_is_given() {{
    let service = InMemory{name}::default();
    assert_eq!(service.get(\"a\"), None);
    service.set(\"a\", \"1\".to_owned());
    assert_eq!(service.get(\"a\").as_deref(), Some(\"1\"));
}}
"
    )
}

/// `lib.rs` with `insertion` placed after the last module declaration (or
/// after the imports), once.
fn declare_module(lib: &str, declaration: &str) -> String {
    let lines: Vec<&str> = lib.lines().collect();
    let after = lines
        .iter()
        .rposition(|line| {
            let line = line.trim_start();
            line.starts_with("pub mod ") || line.starts_with("mod ") || line.contains("rsx_mod!(")
        })
        .or_else(|| lines.iter().rposition(|line| line.starts_with("use ")))
        .map_or(0, |index| index + 1);
    let mut out: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    out.insert(after, declaration.to_owned());
    out.join("\n") + "\n"
}

/// `lib.rs` with `entry` added to `previews()`'s list.
fn add_preview(lib: &str, entry: &str) -> Option<String> {
    let function = lib.find("pub fn previews()")?;
    let open = function + lib[function..].find("vec![")? + "vec![".len();
    Some(format!("{}\n        {entry},{}", &lib[..open], &lib[open..]))
}

/// `lib.rs` with `route` added to `router()`, which is created on first use.
fn add_route(lib: &str, name: &str, route: &str) -> String {
    const MARKER: &str = "        // rustnative:routes";
    let mut lib = lib.to_owned();
    if !lib.contains(MARKER) {
        lib.push_str(
            "
/// The application's routes: `rustnative generate screen` adds to them.
///
/// # Errors
///
/// A route pattern that does not parse.
pub fn router() -> Result<rustnative_core::navigation::Router, rustnative_core::navigation::RouteError> {
    Ok(rustnative_core::navigation::Router::new())
        // rustnative:routes
}
",
        );
    }
    lib.replace(
        MARKER,
        &format!("        .and_then(|router| router.route(\"{name}\", \"{route}\"))\n{MARKER}"),
    )
}

/// Runs `generate` in the project around `here`.
///
/// # Errors
///
/// The name is not usable, a file exists, or the project has no library.
pub fn run(here: &Path, what: &Generate) -> Result<()> {
    let project = Project::find(here)?;
    let root = &project.root;
    if let Generate::Kit { kit } = what {
        return crate::kits::generate(root, *kit);
    }
    let crate_name = project.config.app.name.replace('-', "_");
    let markup = markup(root);
    let lib_path = root.join("src").join("lib.rs");
    let mut lib = std::fs::read_to_string(&lib_path).map_err(|_| {
        Error::Usage(
            "the project has no src/lib.rs — `rustnative generate` adds to the library the \
             current templates make (the application in src/lib.rs, a thin src/main.rs)"
                .into(),
        )
    })?;
    let (name, route) = match what {
        Generate::Component { name } | Generate::Service { name } => (name.clone(), None),
        Generate::Screen { name, route } => (name.clone(), Some(route.clone())),
        Generate::Kit { .. } => unreachable!("kits return above"),
        Generate::ServerResource { .. } => {
            return Err(Error::Usage(
                "`server-resource` generates against the server application model, which this \
                 project does not use: add a server crate first (`PLAN.md` Milestone 49)"
                    .into(),
            ));
        }
    };
    valid(&name)?;
    let module = snake(&name);
    let key = module.replace('_', "-");
    let (source, declaration): (PathBuf, String) = match (what, markup) {
        (Generate::Service { .. }, _) | (_, false) => {
            (root.join("src").join(format!("{module}.rs")), format!("pub mod {module};"))
        }
        (_, true) => (
            root.join("src").join(format!("{module}.rsx")),
            format!("rustnative_core::rsx_mod!(pub {module});"),
        ),
    };
    let test = root.join("tests").join(format!("{module}.rs"));
    if matches!(what, Generate::Service { .. }) {
        write_new(&source, &service(&name))?;
        write_new(&test, &service_test(&crate_name, &module, &name))?;
    } else {
        write_new(&source, &component(&name, &key, markup, route.as_deref()))?;
        write_new(&test, &component_test(&crate_name, &module, &name, &key, route.as_deref()))?;
        let preview = format!("Preview::component::<{module}::{name}>(\"{key}\", ())");
        lib = add_preview(&lib, &preview).unwrap_or(lib);
    }
    lib = declare_module(&lib, &declaration);
    if let Some(route) = &route {
        lib = add_route(&lib, &module, route);
    }
    std::fs::write(&lib_path, lib)
        .map_err(|cause| Error::Io { what: format!("write {}", lib_path.display()), cause })?;
    println!("generate: registered `{module}` in src/lib.rs");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: &str = "//! App.\n\nuse rustnative_core::preview::{Preview, PreviewMatrix};\n\npub mod existing;\n\npub fn previews() -> Vec<Preview> {\n    vec![Preview::component::<App>(\"app\", ())]\n}\n";

    #[test]
    fn names_and_registrations_land_where_the_project_declares_things() {
        assert_eq!(snake("UserList"), "user_list");
        assert!(valid("Settings").is_ok());
        assert!(valid("settings").is_err() && valid("Bad Name").is_err());

        let lib = declare_module(LIB, "pub mod settings;");
        assert!(lib.contains("pub mod existing;\npub mod settings;\n"), "{lib}");
        let lib = add_preview(&lib, "Preview::component::<settings::Settings>(\"settings\", ())")
            .unwrap();
        assert!(lib.contains("vec![\n        Preview::component::<settings::Settings>(\"settings\", ()),Preview::component::<App>"));
        let lib = add_route(&lib, "settings", "/settings");
        let lib = add_route(&lib, "user_list", "/users");
        assert_eq!(lib.matches("pub fn router()").count(), 1, "created once");
        let settings = lib.find("route(\"settings\"").unwrap();
        assert!(settings < lib.find("route(\"user_list\"").unwrap(), "in the order generated");
    }

    #[test]
    fn a_component_is_written_in_the_project_s_syntax() {
        let builder = component("Settings", "settings", false, None);
        assert!(builder.contains("Node::column(\"settings\""));
        let markup = component("Settings", "settings", true, Some("/settings"));
        assert!(markup.contains("<Column key=\"settings\">"));
        assert!(markup.contains("pub const ROUTE: &str = \"/settings\";"));
    }
}
