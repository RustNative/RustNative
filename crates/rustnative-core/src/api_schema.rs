//! API schemas from types (`PLAN.md` Milestone 49, `C35`): the JSON Schema
//! of a value, which the server's `OpenAPI` document, request validation,
//! generated clients, and contract tests are built from.
//!
//! Standard types have their schema here; an application type states its
//! own with [`object`] — no derive needed:
//!
//! ```
//! use rustnative_core::api_schema::{ApiSchema, object};
//!
//! struct Note { title: String, pinned: bool, tags: Vec<String> }
//! impl ApiSchema for Note {
//!     fn schema() -> serde_json::Value {
//!         object("Note", [("title", String::schema(), true), ("pinned", bool::schema(), true), ("tags", Vec::<String>::schema(), false)])
//!     }
//! }
//! assert_eq!(Note::schema()["required"], serde_json::json!(["title", "pinned"]));
//! ```

use serde_json::{Value, json};

/// A type's JSON Schema.
pub trait ApiSchema {
    /// The schema.
    fn schema() -> Value;
}

/// An object schema named `title` with `(name, schema, required)` fields.
#[must_use]
pub fn object<'a>(title: &str, fields: impl IntoIterator<Item = (&'a str, Value, bool)>) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, schema, is_required) in fields {
        properties.insert(name.to_owned(), schema);
        if is_required {
            required.push(Value::String(name.to_owned()));
        }
    }
    json!({ "title": title, "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

macro_rules! scalar {
    ($schema:expr, $($type:ty),*) => {
        $(impl ApiSchema for $type {
            fn schema() -> Value {
                $schema
            }
        })*
    };
}

scalar!(json!({ "type": "string" }), String, str);
scalar!(json!({ "type": "boolean" }), bool);
scalar!(json!({ "type": "integer" }), i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);
scalar!(json!({ "type": "number" }), f32, f64);
scalar!(json!({ "type": "null" }), ());

impl<T: ApiSchema> ApiSchema for Vec<T> {
    fn schema() -> Value {
        json!({ "type": "array", "items": T::schema() })
    }
}

impl<T: ApiSchema> ApiSchema for Option<T> {
    fn schema() -> Value {
        json!({ "anyOf": [T::schema(), { "type": "null" }] })
    }
}

impl ApiSchema for Value {
    fn schema() -> Value {
        json!({})
    }
}
