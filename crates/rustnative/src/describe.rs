//! `rustnative describe --json` (`PLAN.md` Milestone 52): a
//! machine-readable description of the framework, generated from the same
//! sources the compiler and the reference use, so a tool that writes code
//! for it writes correct code rather than plausible code.
//!
//! It lists:
//!
//! - every markup element, with each attribute and the builder method it
//!   calls;
//! - the utility classes, with the style properties each one sets, and the
//!   variants;
//! - the capabilities, events, and services;
//! - the component contract, and the layout semantics.
//!
//! `docs/api/framework.json` is its committed output, and a test fails
//! when that file goes stale.

use serde_json::{Value, json};

/// The description.
#[must_use]
pub fn description() -> Value {
    let elements: Vec<Value> = rustnative_markup::table::element_table()
        .into_iter()
        .map(|element| {
            json!({
                "name": element.name,
                "constructor": format!("{:?}", element.constructor),
                "children": element.children,
                "attributes": element.attrs.iter().map(|attribute| json!({
                    "name": attribute.name,
                    "kind": format!("{:?}", attribute.kind),
                    "builder": attribute.method,
                    "required": attribute.required,
                    "flag": attribute.flag,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    let vocabulary = rustnative_style::vocabulary::Vocabulary::defaults();
    let mut utilities: Vec<Value> = vocabulary
        .class_names()
        .into_iter()
        .filter_map(|class| {
            let declarations = vocabulary.resolve_classes(&class).ok()?;
            let mut properties: Vec<String> = declarations
                .iter()
                .map(|declaration| declaration.declaration.property.to_string())
                .collect();
            properties.dedup();
            Some(json!({ "class": class, "sets": properties }))
        })
        .collect();
    utilities.sort_by(|a, b| a["class"].as_str().cmp(&b["class"].as_str()));

    json!({
        "framework": rustnative_core::package::FRAMEWORK_VERSION,
        "syntaxes": {
            "builder": "Node::<constructor>(key, …) with with_* modifiers",
            "markup": "rsx! bodies and .rsx files; each element lowers to the constructor named here",
        },
        "elements": elements,
        "utilities": utilities,
        "variants": vocabulary.variant_names(),
        "capabilities": rustnative_core::capability::Capability::ALL.iter().map(|capability| format!("{capability:?}")).collect::<Vec<_>>(),
        "events": rustnative_core::event::EVENT_NAMES,
        "services": SERVICES,
        "component": {
            "trait": "rustnative_core::Component",
            "types": ["Props: PartialEq + Clone", "Message"],
            "methods": [
                "fn new(props: Self::Props) -> Self",
                "fn props(&self) -> &Self::Props",
                "fn set_props(&mut self, props: Self::Props)",
                "fn view(&self) -> Node",
                "fn update(&mut self, event: Event)",
                "fn render(&mut self, context: &mut ComponentContext<'_, Self::Message>) -> Node (default: view)",
            ],
            "rules": [
                "state changes only in update, in answer to an event or a message",
                "render is a function of state, props, and what the context provides",
                "keys name nodes; events and queries find nodes by key",
            ],
        },
        "layout": [
            "column and row stack children along their axis, with gap and padding in logical start/end",
            "grid places children in named tracks",
            "width and height are Fixed, Fill, or Content; Fill shares what is left",
            "constraints clamp a node's size; margins are outside it",
            "direction follows the environment (LTR or RTL); start and end mirror",
        ],
    })
}

/// The services an application registers (`Services::with_…`).
const SERVICES: [&str; 14] = [
    "http",
    "storage",
    "clipboard",
    "file_dialogs",
    "system",
    "state_store",
    "permissions",
    "locale",
    "printing",
    "serial",
    "secure_storage",
    "push",
    "commerce",
    "surfaces and flags (always present)",
];

/// Prints the description.
pub fn run(json: bool) {
    let description = description();
    if json {
        println!("{}", serde_json::to_string_pretty(&description).unwrap_or_default());
    } else {
        println!(
            "Rust Native {}: {} elements, {} utility classes, {} capabilities, {} events. `--json` prints them all.",
            description["framework"].as_str().unwrap_or_default(),
            description["elements"].as_array().map_or(0, Vec::len),
            description["utilities"].as_array().map_or(0, Vec::len),
            description["capabilities"].as_array().map_or(0, Vec::len),
            description["events"].as_array().map_or(0, Vec::len),
        );
    }
}
