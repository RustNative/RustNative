//! Custom elements (`C43-1`): a client component exported as a web
//! component any page can use — with no other Rust Native code on it.
//!
//! ```
//! # #[rustnative_web::client]
//! # pub mod counter {
//! #     use rustnative_core::{Event, Node};
//! #     use rustnative_web::Effects;
//! #     #[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
//! #     pub struct Counter { pub count: i32, pub label: String }
//! #     impl Counter {
//! #         pub fn update(&mut self, _event: Event, _fx: &mut Effects<()>) {}
//! #         pub fn view(&self) -> Node { Node::label("count", format!("{}", self.count)) }
//! #     }
//! # }
//! use rustnative_web::element::CustomElement;
//!
//! let element = CustomElement::new::<counter::Counter>("rn-counter");
//! // `<rn-counter count="3" label="Apples">`: the state's fields are the
//! // element's attributes and properties.
//! assert_eq!(element.fields(), ["count", "label"]);
//! assert!(element.files().iter().any(|(path, _)| path == "/_rn/e/rn-counter.js"));
//! ```
//!
//! The element renders in light DOM, so a `<label for>` or `aria-*`
//! reference across it keeps working, and it is styled by the rules it
//! brings (applied through the CSS object model, which a strict policy
//! allows). Values the component publishes (`fx.publish(topic, value)`) are
//! DOM events named `topic` on the element, with the value as `detail`.

use crate::client::{ClientLogic, ClientModule, state_json};

/// A client component as a custom element.
#[derive(Debug, Clone)]
pub struct CustomElement {
    tag: String,
    module: &'static ClientModule,
    initial: serde_json::Value,
}

impl CustomElement {
    /// Component `S` as `<tag>`, starting from `S::default()`.
    ///
    /// # Panics
    ///
    /// `tag` is not a valid custom element name (lower case, starting with
    /// a letter, with a hyphen) — a mistake in the source.
    #[must_use]
    pub fn new<S: ClientLogic + Default>(tag: &str) -> Self {
        let valid = tag.contains('-')
            && tag.starts_with(|first: char| first.is_ascii_lowercase())
            && tag.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '-' | '.' | '_')
            });
        assert!(
            valid,
            "{tag:?} is not a custom element name: lower case, starting with a letter, with a hyphen"
        );
        let initial = state_json(&S::default()).unwrap_or(serde_json::Value::Null);
        Self { tag: tag.to_owned(), module: S::MODULE, initial }
    }

    /// Its tag.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The state's fields: its attributes (kebab-case) and properties.
    #[must_use]
    pub fn fields(&self) -> Vec<String> {
        self.initial.as_object().map(|fields| fields.keys().cloned().collect()).unwrap_or_default()
    }

    /// The module a page loads to define it: `/_rn/e/{tag}.js`.
    #[must_use]
    pub fn script(&self) -> String {
        let runtime = crate::runtime::runtime_url("../");
        let module = self.module.url("../");
        let fns: serde_json::Map<String, serde_json::Value> = self
            .module
            .server_fns
            .iter()
            .map(|(name, path)| ((*name).to_owned(), serde_json::json!(path)))
            .collect();
        format!(
            "import {{ defineElement }} from {runtime};\nimport make from {module};\nawait defineElement({tag}, make, {initial}, {fns}, {css});\n",
            runtime = serde_json::Value::String(runtime),
            module = serde_json::Value::String(module),
            tag = serde_json::Value::String(self.tag.clone()),
            initial = self.initial,
            fns = serde_json::Value::Object(fns),
            css = serde_json::Value::String(self.base_css()),
        )
    }

    /// The framework's base and theme rules and the tokens the component
    /// refers to: what a page with no other Rust Native code lacks.
    fn base_css(&self) -> String {
        let mut sheet = crate::css::StyleSheet::new();
        for token in self.module.tokens {
            sheet.note_token(token);
        }
        sheet.css(&rustnative_core::Theme::default())
    }

    /// Every file a host serves for it, by path: its script, the runtime,
    /// and the component's module.
    #[must_use]
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        vec![
            (format!("/_rn/e/{}.js", self.tag), self.script().into_bytes()),
            (crate::runtime::runtime_url("/_rn/"), crate::runtime::RUNTIME_JS.as_bytes().to_vec()),
            (self.module.url("/_rn/"), self.module.js.as_bytes().to_vec()),
        ]
    }
}
