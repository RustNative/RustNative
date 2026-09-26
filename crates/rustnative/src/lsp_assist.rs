//! What `rustnative lsp` answers itself, in `.rsx` files and inside `rsx!`
//! in `.rs` files alike (`PLAN.md` Milestone 43): neither syntax gets
//! tooling the other lacks.
//!
//! - **Markup**: attribute completion, hover that names the builder method
//!   an attribute calls, and go-to-definition to that method.
//! - **Class strings** (`class="…"`, `style="…"`, `classes!("…")`,
//!   `styles!("…")`): completion of the vocabulary's classes, and hover that
//!   lists the properties a class sets and the token each value references
//!   (and what that token is in this project's `app.css`).
//! - **Diagnostics**: one that names a class or an attribute is narrowed to
//!   the word it names, not the whole string or macro call around it.
//! - **Structural editing** (`C56`): `rustnative/setAttribute`,
//!   `rustnative/insertElement`, `rustnative/removeElement`, and
//!   `rustnative/moveElement` answer with a workspace edit that changes
//!   only the element they are about (`crate::markup_edit`).

use std::path::{Path, PathBuf};

use rustnative_style::Vocabulary;
use serde_json::{Value, json};

use crate::markup_edit::{self, ElementRange};

/// The byte offset of LSP position `(line, character)` (characters counted
/// as the text's characters).
#[must_use]
pub fn offset(text: &str, line: usize, character: usize) -> Option<usize> {
    let mut start = 0;
    for (index, current) in text.split_inclusive('\n').enumerate() {
        if index == line {
            let within: usize = current.chars().take(character).map(char::len_utf8).sum();
            return Some(start + within.min(current.len()));
        }
        start += current.len();
    }
    (line == text.split_inclusive('\n').count()).then_some(text.len())
}

/// The LSP position of byte `offset`.
#[must_use]
pub fn position(text: &str, offset: usize) -> Value {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count();
    let character = before.rsplit('\n').next().map_or(0, |last| last.chars().count());
    json!({ "line": line, "character": character })
}

fn range(text: &str, start: usize, end: usize) -> Value {
    json!({ "start": position(text, start), "end": position(text, end) })
}

/// A class or declaration string: where its content is, and which it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassString {
    /// The content's first byte (just inside the quote).
    pub start: usize,
    /// Just past its content (the closing quote).
    pub end: usize,
    /// A declaration block (`style=`, `styles!`) rather than classes.
    pub declarations: bool,
}

/// The class or declaration string `offset` is inside, if any.
#[must_use]
pub fn class_string_at(text: &str, offset: usize, rsx_file: bool) -> Option<ClassString> {
    let literal = |value: (usize, usize)| {
        (text.as_bytes().get(value.0) == Some(&b'"') && value.1 > value.0 + 1)
            .then_some((value.0 + 1, value.1 - 1))
    };
    // An attribute of an element.
    let elements = markup_edit::scan(text, rsx_file);
    if let Some(element) = markup_edit::element_at(&elements, offset) {
        for attr in &element.attrs {
            if let Some((start, end)) = literal(attr.value) {
                if (start..=end).contains(&offset) && (attr.name == "class" || attr.name == "style")
                {
                    return Some(ClassString { start, end, declarations: attr.name == "style" });
                }
            }
        }
    }
    // A macro call.
    for (name, declarations) in [("classes!(\"", false), ("styles!(\"", true)] {
        let mut from = 0;
        while let Some(found) = text[from..].find(name) {
            let start = from + found + name.len();
            let end = start + text[start..].find('"').unwrap_or(text.len() - start);
            if (start..=end).contains(&offset) {
                return Some(ClassString { start, end, declarations });
            }
            from = end;
        }
    }
    None
}

/// The class under `offset` in `string` (whitespace-separated), and the part
/// before the cursor.
#[must_use]
pub fn class_at(text: &str, string: ClassString, offset: usize) -> (String, String) {
    let content = &text[string.start..string.end];
    let at = offset.saturating_sub(string.start).min(content.len());
    let separator = |c: char| c.is_whitespace() || (string.declarations && c == ';');
    let start = content[..at].rfind(separator).map_or(0, |found| found + 1);
    let end = content[at..].find(separator).map_or(content.len(), |found| at + found);
    (content[start..end].to_owned(), content[start..at].to_owned())
}

/// The vocabulary for a file: the default theme with the project's
/// `app.css` (the nearest one above the file), as the build compiles it.
#[must_use]
pub fn vocabulary_for(path: &Path) -> Vocabulary {
    path.ancestors()
        .map(|folder| folder.join("app.css"))
        .find(|file| file.is_file())
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|source| Vocabulary::with_style_file(&source).ok())
        .unwrap_or_else(Vocabulary::defaults)
}

/// Completion items for a class being typed: `prefix` is what is typed of
/// it so far, variants (`hover:`) kept.
#[must_use]
pub fn class_completion(vocabulary: &Vocabulary, prefix: &str) -> Value {
    let (variants, typed) =
        prefix.rsplit_once(':').map_or(("", prefix), |(before, after)| (before, after));
    let items: Vec<Value> = vocabulary
        .class_names()
        .into_iter()
        .filter(|name| name.starts_with(typed))
        .map(|name| {
            let label = if variants.is_empty() { name } else { format!("{variants}:{name}") };
            json!({ "label": label, "kind": 12, "detail": "class" })
        })
        .collect();
    json!(items)
}

/// Hover for a class: every property it sets and each token value
/// resolved.
#[must_use]
pub fn class_hover(vocabulary: &Vocabulary, class: &str, declarations: bool) -> Option<Value> {
    let resolved = if declarations {
        vocabulary.resolve_declarations(class)
    } else {
        vocabulary.resolve_classes(class)
    };
    let declarations = match resolved {
        Ok(declarations) if !declarations.is_empty() => declarations,
        Ok(_) => return None,
        Err(errors) => {
            let message =
                errors.iter().map(|error| error.message.clone()).collect::<Vec<_>>().join("\n");
            return Some(json!({ "contents": { "kind": "markdown", "value": message } }));
        }
    };
    let tokens = vocabulary.token_table();
    let lines: Vec<String> = declarations
        .iter()
        .map(|declaration| {
            let value = &declaration.declaration.value;
            let resolved = tokens
                .resolve(value)
                .map(|resolved| resolved.to_string())
                .filter(|resolved| *resolved != value.to_string())
                .map(|resolved| format!(" = `{resolved}`"))
                .unwrap_or_default();
            format!(
                "- `{}{}: {value}`{resolved}",
                declaration.condition, declaration.declaration.property
            )
        })
        .collect();
    Some(
        json!({ "contents": { "kind": "markdown", "value": format!("`{class}` sets:\n{}", lines.join("\n")) } }),
    )
}

/// The attribute of `element` whose name `offset` is on.
#[must_use]
pub fn attribute_at(element: &ElementRange, offset: usize) -> Option<&markup_edit::AttrRange> {
    element
        .attrs
        .iter()
        .find(|attr| (attr.name_start..=attr.name_start + attr.name.len()).contains(&offset))
}

/// Where `rustnative-core`'s source is, for a project around `path`.
fn framework_source(path: &Path) -> Option<PathBuf> {
    let folder = path.parent()?;
    let output =
        std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(folder)
            .args(["metadata", "--format-version", "1"])
            .output()
            .ok()?;
    let metadata: Value = serde_json::from_slice(&output.stdout).ok()?;
    let manifest = metadata["packages"]
        .as_array()?
        .iter()
        .find(|package| package["name"] == "rustnative-core")?["manifest_path"]
        .as_str()?
        .to_owned();
    Some(Path::new(&manifest).parent()?.join("src"))
}

/// The definition of a builder method named as the element table names it
/// (`LayoutStyle::width`, `Node::with_class(…)`): the `fn` in the framework's
/// source, in the file that has an `impl` of its type.
#[must_use]
pub fn builder_method_location(method: &str, path: &Path) -> Option<Value> {
    let path_part = method.split('(').next()?;
    let (type_name, name) = path_part.rsplit_once("::")?;
    let source = framework_source(path)?;
    let mut files = Vec::new();
    collect_rust(&source, &mut files);
    let pattern = [format!("pub fn {name}("), format!("pub const fn {name}(")];
    let mut best: Option<(bool, PathBuf, usize)> = None;
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let has_impl = text.contains(&format!("impl {type_name} {{"));
        if let Some(at) = pattern.iter().filter_map(|pattern| text.find(pattern)).min() {
            if best.as_ref().is_none_or(|(impl_found, _, _)| has_impl && !impl_found) {
                best = Some((has_impl, file, at));
            }
        }
    }
    let (_, file, at) = best?;
    let text = std::fs::read_to_string(&file).ok()?;
    let uri = format!(
        "file:///{}",
        file.display().to_string().replace('\\', "/").trim_start_matches('/')
    );
    Some(json!({ "uri": uri, "range": range(&text, at, at) }))
}

fn collect_rust(folder: &Path, into: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            into.push(path);
        }
    }
}

/// A diagnostic's range narrowed to the first backticked word of its
/// message that occurs inside the range — the class or attribute it is
/// about, rather than the whole string or call.
#[must_use]
pub fn narrow(text: &str, diagnostic: &Value) -> Option<Value> {
    let message = diagnostic.get("message")?.as_str()?;
    let spanned_range = diagnostic.get("range")?;
    let start = offset(
        text,
        usize::try_from(spanned_range["start"]["line"].as_u64()?).ok()?,
        usize::try_from(spanned_range["start"]["character"].as_u64()?).ok()?,
    )?;
    let end = offset(
        text,
        usize::try_from(spanned_range["end"]["line"].as_u64()?).ok()?,
        usize::try_from(spanned_range["end"]["character"].as_u64()?).ok()?,
    )?;
    let spanned = text.get(start..end)?;
    let named = message
        .split('`')
        .skip(1)
        .step_by(2)
        .find(|word| !word.is_empty() && spanned.contains(*word))?;
    if named.len() == spanned.len() {
        return None;
    }
    let at = start + spanned.find(named)?;
    Some(range(text, at, at + named.len()))
}

/// Answers a structural editing request with a workspace edit.
///
/// # Errors
///
/// The request names no element, or the edit cannot be made.
pub fn structural(
    method: &str,
    params: &Value,
    text: &str,
    rsx_file: bool,
) -> Result<Value, String> {
    let uri = params.pointer("/textDocument/uri").and_then(Value::as_str).ok_or("no document")?;
    let elements = markup_edit::scan(text, rsx_file);
    let element_at = |key: &str| -> Result<&ElementRange, String> {
        let position = params.get(key).ok_or_else(|| format!("no `{key}`"))?;
        let line =
            usize::try_from(position["line"].as_u64().ok_or("no line")?).map_err(|_| "line")?;
        let character = usize::try_from(position["character"].as_u64().ok_or("no character")?)
            .map_err(|_| "character")?;
        let at = offset(text, line, character).ok_or("the position is outside the document")?;
        markup_edit::element_at(&elements, at).ok_or_else(|| "no element there".to_owned())
    };
    let string =
        |key: &str| params.get(key).and_then(Value::as_str).ok_or_else(|| format!("no `{key}`"));
    let index = || {
        usize::try_from(params.get("index").and_then(Value::as_u64).unwrap_or(u64::MAX))
            .unwrap_or(usize::MAX)
    };
    let edits = match method {
        "rustnative/setAttribute" => {
            vec![markup_edit::set_attribute(
                text,
                element_at("position")?,
                string("name")?,
                string("value")?,
            )]
        }
        "rustnative/removeElement" => {
            vec![markup_edit::remove_element(text, element_at("position")?)]
        }
        "rustnative/insertElement" => {
            vec![markup_edit::insert_element(
                text,
                element_at("parent")?,
                index(),
                string("markup")?,
            )?]
        }
        "rustnative/moveElement" => markup_edit::move_element(
            text,
            element_at("position")?,
            element_at("parent")?,
            index(),
        )?,
        other => return Err(format!("`{other}` is not a structural edit")),
    };
    let text_edits: Vec<Value> = edits
        .iter()
        .map(|edit| json!({ "range": range(text, edit.start, edit.end), "newText": edit.new_text }))
        .collect();
    Ok(json!({ "changes": { uri: text_edits } }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "fn view() -> Node {\n    rsx! {\n        <Column key=\"root\" class=\"p-4 bg-blue-500\">\n            <Label key=\"a\" text=\"A\" />\n        </Column>\n    }\n}\nconst X: DeclarationSet = classes!(\"text-lg font-bold\");\n";

    #[test]
    fn class_strings_are_found_in_attributes_and_macro_calls() {
        let in_attr = FILE.find("bg-blue").unwrap() + 3;
        let string = class_string_at(FILE, in_attr, false).unwrap();
        assert!(!string.declarations);
        assert_eq!(class_at(FILE, string, in_attr), ("bg-blue-500".to_owned(), "bg-".to_owned()));
        let in_macro = FILE.find("font-bold").unwrap() + 4;
        let string = class_string_at(FILE, in_macro, false).unwrap();
        assert_eq!(class_at(FILE, string, in_macro).0, "font-bold");
        assert!(class_string_at(FILE, FILE.find("key=\"a\"").unwrap() + 5, false).is_none());
    }

    #[test]
    fn classes_complete_with_variants_and_hover_names_properties_and_tokens() {
        let vocabulary = Vocabulary::defaults();
        let items = class_completion(&vocabulary, "hover:bg-bl");
        let labels: Vec<&str> =
            items.as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
        assert!(labels.contains(&"hover:bg-blue-500"), "{labels:?}");
        let hover = class_hover(&vocabulary, "bg-blue-500", false).unwrap();
        let text = hover["contents"]["value"].as_str().unwrap();
        assert!(text.contains("background-color: var(--color-blue-500)"), "{text}");
        assert!(text.contains(" = `"), "the token's value: {text}");
        let unknown = class_hover(&vocabulary, "bg-blu-500", false).unwrap();
        assert!(unknown["contents"]["value"].as_str().unwrap().contains("did you mean"));
    }

    #[test]
    fn a_diagnostic_is_narrowed_to_the_class_it_names() {
        let start = FILE.find("\"p-4").unwrap();
        let end = FILE[start + 1..].find('"').unwrap() + start + 2;
        let diagnostic = json!({
            "range": range(FILE, start, end),
            "message": "`bg-blue-500` is not a class in the vocabulary — did you mean `bg-blue-400`?",
        });
        let narrowed = narrow(FILE, &diagnostic).unwrap();
        let at = FILE.find("bg-blue-500").unwrap();
        assert_eq!(narrowed, range(FILE, at, at + "bg-blue-500".len()));
        assert!(
            narrow(FILE, &json!({ "range": range(FILE, at, at + 11), "message": "`bg-blue-500`" }))
                .is_none(),
            "already narrow"
        );
    }

    #[test]
    fn structural_requests_answer_with_edits_to_one_element() {
        let label = position(FILE, FILE.find("<Label").unwrap() + 1);
        let params = json!({ "textDocument": { "uri": "file:///a.rs" }, "position": label, "name": "text", "value": "\"B\"" });
        let edit = structural("rustnative/setAttribute", &params, FILE, false).unwrap();
        let edits = &edit["changes"]["file:///a.rs"];
        assert_eq!(edits.as_array().unwrap().len(), 1);
        assert_eq!(edits[0]["newText"], json!("\"B\""));
        let column = position(FILE, FILE.find("<Column").unwrap() + 1);
        let insert = json!({ "textDocument": { "uri": "file:///a.rs" }, "parent": column, "index": 0, "markup": "<Button key=\"b\" text=\"B\" />" });
        assert!(structural("rustnative/insertElement", &insert, FILE, false).is_ok());
        assert!(structural("rustnative/renameElement", &insert, FILE, false).is_err());
    }

    #[test]
    fn an_attribute_s_definition_is_the_builder_method_it_calls() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lsp_assist.rs");
        let location = builder_method_location("LayoutStyle::width", &here).unwrap();
        let uri = location["uri"].as_str().unwrap();
        assert!(uri.ends_with("rustnative-core/src/layout/constraints.rs"), "{uri}");
        let line = usize::try_from(location["range"]["start"]["line"].as_u64().unwrap()).unwrap();
        let path = uri.trim_start_matches("file:///");
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.lines().nth(line).unwrap().contains("fn width("));
    }
}
