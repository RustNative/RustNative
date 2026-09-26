//! The API schema (`C35`): an `OpenAPI` 3.1 document derived from the
//! routes and the server functions' types, served at `/openapi.json`;
//! a contract check that fails when a published version breaks; and a
//! TypeScript client generated from the document.

use std::fmt::Write as _;

use rustnative_core::api_schema::ApiSchema;
use rustnative_core::server_fn::ServerFn;
use serde_json::{Map, Value, json};

/// One described operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    /// Its path (`OpenAPI` form, `{id}`).
    pub path: String,
    /// Its method, lower case.
    pub method: String,
    /// Its operation id (a server function's path, dotted).
    pub id: String,
    /// The request body's schema.
    pub request: Option<Value>,
    /// The success response's schema.
    pub response: Value,
}

impl Operation {
    /// A server function's operation.
    #[must_use]
    pub fn function<F: ServerFn>(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            method: "post".into(),
            id: F::PATH.replace('/', "."),
            request: Some(F::Input::schema()),
            response: F::Output::schema(),
        }
    }
}

/// An `OpenAPI` path from a route pattern: `:id` becomes `{id}`.
fn openapi_path(pattern: &str) -> (String, Vec<String>) {
    let mut names = Vec::new();
    let path = pattern
        .split('/')
        .map(|segment| {
            if let Some(name) = segment.strip_prefix(':').or_else(|| segment.strip_prefix('*')) {
                names.push(name.to_owned());
                format!("{{{name}}}")
            } else {
                segment.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    (path, names)
}

/// The document for `routes` and `operations`.
#[must_use]
pub fn document(
    title: &str,
    version: &str,
    routes: &[crate::RouteInfo],
    operations: &[Operation],
) -> Value {
    let mut paths = Map::new();
    for route in routes {
        let (path, parameters) = openapi_path(&route.pattern);
        let entry = paths.entry(path.clone()).or_insert_with(|| json!({}));
        for method in &route.methods {
            let method = method.as_str().to_lowercase();
            let described = operations
                .iter()
                .find(|operation| operation.path == route.pattern && operation.method == method);
            let mut operation = json!({
                "parameters": parameters.iter().map(|name| json!({ "name": name, "in": "path", "required": true, "schema": { "type": "string" } })).collect::<Vec<_>>(),
                "responses": { "200": { "description": "OK" } },
                "x-access": route.access,
            });
            if route.access != "public" {
                operation["security"] = json!([{ "session": [] }, { "bearer": [] }]);
            }
            if let Some(described) = described {
                operation["operationId"] = json!(described.id);
                if let Some(request) = &described.request {
                    operation["requestBody"] = json!({ "required": true, "content": { "application/json": { "schema": request } } });
                }
                operation["responses"]["200"]["content"] =
                    json!({ "application/json": { "schema": described.response } });
            }
            entry[method.as_str()] = operation;
        }
    }
    json!({
        "openapi": "3.1.0",
        "info": { "title": title, "version": version },
        "paths": paths,
        "components": { "securitySchemes": {
            "session": { "type": "apiKey", "in": "cookie", "name": crate::auth::session::COOKIE },
            "bearer": { "type": "http", "scheme": "bearer" },
        } },
    })
}

fn required(schema: &Value) -> Vec<String> {
    schema["required"].as_array().map_or_else(Vec::new, |names| {
        names.iter().filter_map(|name| name.as_str().map(str::to_owned)).collect()
    })
}

fn properties(schema: &Value) -> Map<String, Value> {
    schema["properties"].as_object().cloned().unwrap_or_default()
}

/// What in `current` breaks a client written against `published`:
/// operations removed, request fields newly required, response fields
/// removed, and field types changed. Empty when `current` is compatible.
#[must_use]
pub fn breaking_changes(published: &Value, current: &Value) -> Vec<String> {
    let mut breaks = Vec::new();
    let Some(paths) = published["paths"].as_object() else { return breaks };
    for (path, methods) in paths {
        let Some(methods) = methods.as_object() else { continue };
        for (method, old) in methods {
            let new = &current["paths"][path][method];
            if new.is_null() {
                breaks.push(format!("{} {path} was removed", method.to_uppercase()));
                continue;
            }
            let old_request = &old["requestBody"]["content"]["application/json"]["schema"];
            let new_request = &new["requestBody"]["content"]["application/json"]["schema"];
            let was_required = required(old_request);
            for field in required(new_request) {
                if !was_required.contains(&field) {
                    breaks.push(format!(
                        "{} {path}: request field `{field}` is now required",
                        method.to_uppercase()
                    ));
                }
            }
            let old_response = &old["responses"]["200"]["content"]["application/json"]["schema"];
            let new_response = &new["responses"]["200"]["content"]["application/json"]["schema"];
            let new_fields = properties(new_response);
            for (field, schema) in properties(old_response) {
                match new_fields.get(&field) {
                    None => breaks.push(format!(
                        "{} {path}: response field `{field}` was removed",
                        method.to_uppercase()
                    )),
                    Some(now) if now["type"] != schema["type"] => breaks.push(format!(
                        "{} {path}: response field `{field}` changed type",
                        method.to_uppercase()
                    )),
                    Some(_) => {}
                }
            }
            if old_response["type"] != new_response["type"] && !old_response.is_null() {
                breaks.push(format!("{} {path}: the response changed type", method.to_uppercase()));
            }
        }
    }
    breaks
}

fn typescript_type(schema: &Value, declarations: &mut Vec<String>) -> String {
    if let Some(options) = schema["anyOf"].as_array() {
        return options
            .iter()
            .map(|option| typescript_type(option, declarations))
            .collect::<Vec<_>>()
            .join(" | ");
    }
    match schema["type"].as_str() {
        Some("string") => "string".into(),
        Some("integer" | "number") => "number".into(),
        Some("boolean") => "boolean".into(),
        Some("null") => "null".into(),
        Some("array") => format!("Array<{}>", typescript_type(&schema["items"], declarations)),
        Some("object") => {
            let name = schema["title"].as_str().unwrap_or("Object").to_owned();
            let needed = required(schema);
            let mut body = String::new();
            for (field, field_schema) in properties(schema) {
                let optional = if needed.contains(&field) { "" } else { "?" };
                let _ = writeln!(
                    body,
                    "  {field}{optional}: {};",
                    typescript_type(&field_schema, declarations)
                );
            }
            let declaration = format!("export interface {name} {{\n{body}}}\n");
            if !declarations.contains(&declaration) {
                declarations.push(declaration);
            }
            name
        }
        _ => "unknown".into(),
    }
}

/// A TypeScript client for the document's operations with ids: one
/// `async` function per server function, typed by its schemas.
#[must_use]
pub fn typescript_client(document: &Value) -> String {
    let mut declarations = Vec::new();
    let mut functions = String::new();
    for (path, methods) in document["paths"].as_object().into_iter().flatten() {
        for (method, operation) in methods.as_object().into_iter().flatten() {
            let Some(id) = operation["operationId"].as_str() else { continue };
            let name = id
                .split('.')
                .enumerate()
                .map(|(index, part)| {
                    if index == 0 { part.to_owned() } else { part[..1].to_uppercase() + &part[1..] }
                })
                .collect::<String>()
                .replace('-', "_");
            let input = typescript_type(
                &operation["requestBody"]["content"]["application/json"]["schema"],
                &mut declarations,
            );
            let output = typescript_type(
                &operation["responses"]["200"]["content"]["application/json"]["schema"],
                &mut declarations,
            );
            let _ = write!(
                functions,
                "export async function {name}(base: string, input: {input}, headers: Record<string, string> = {{}}): Promise<{output}> {{\n  \
                 const response = await fetch(base + \"{path}\", {{ method: \"{}\", headers: {{ \"content-type\": \"application/json\", accept: \"application/json\", ...headers }}, body: JSON.stringify(input) }});\n  \
                 if (!response.ok) throw new Error((await response.json()).error ?? response.statusText);\n  \
                 return await response.json();\n}}\n\n",
                method.to_uppercase()
            );
        }
    }
    format!(
        "// Generated from the server's OpenAPI document; do not edit.\n\n{}\n{functions}",
        declarations.join("\n")
    )
}
