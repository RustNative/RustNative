//! Where markup is in a file, element by element, and edits to it that
//! touch nothing else (`PLAN.md` Milestone 43, `C56`): the structural
//! editing API a visual designer can drive through `rustnative lsp`, and
//! what the language server uses to find the element, attribute, or class
//! string under the cursor.
//!
//! The scanner works on text, not tokens, so the edits are text edits of
//! exactly the element or attribute they change. Formatting and comments
//! outside the edited span are untouched. It finds markup where the
//! compiler does: in a `.rsx` file, an element begins with `<` and a
//! capitalized name after an opening delimiter, a comma, `=`, `>`, a
//! closure's `|`, or the file's start; in a `.rs` file, only inside an
//! `rsx!` invocation.

/// One attribute, by byte offsets into the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrRange {
    /// Its name.
    pub name: String,
    /// Where its name starts.
    pub name_start: usize,
    /// Where its value starts and ends (the quotes or braces included),
    /// or its name's end for a flag written without one.
    pub value: (usize, usize),
}

/// One element, by byte offsets into the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementRange {
    /// Its name as written.
    pub name: String,
    /// Where its `<` is.
    pub start: usize,
    /// Just past its opening tag's `>`.
    pub open_end: usize,
    /// Just past its last `>` (the closing tag's, or the self-closing one).
    pub end: usize,
    /// Whether it closes itself.
    pub self_closing: bool,
    /// Its attributes.
    pub attrs: Vec<AttrRange>,
    /// Its child elements (including ones inside braced expressions).
    pub children: Vec<ElementRange>,
}

impl ElementRange {
    /// The innermost element whose extent contains `offset`, starting here.
    #[must_use]
    pub fn at(&self, offset: usize) -> Option<&Self> {
        if offset < self.start || offset >= self.end {
            return None;
        }
        self.children.iter().find_map(|child| child.at(offset)).or(Some(self))
    }
}

/// The regions of `text` that hold markup: all of it for a `.rsx` file,
/// the bodies of `rsx!` invocations for a `.rs` file.
#[must_use]
pub fn regions(text: &str, rsx_file: bool) -> Vec<(usize, usize)> {
    if rsx_file {
        return vec![(0, text.len())];
    }
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("rsx!") {
        let at = from + found + 4;
        from = at;
        let Some(open) = text[at..].find(|c: char| !c.is_whitespace()).map(|skip| at + skip) else {
            break;
        };
        let (opening, closing) = match bytes[open] {
            b'{' => (b'{', b'}'),
            b'(' => (b'(', b')'),
            b'[' => (b'[', b']'),
            _ => continue,
        };
        let mut depth = 0_i32;
        let mut index = open;
        while index < bytes.len() {
            if bytes[index] == b'"' {
                index = skip_string(bytes, index);
                continue;
            }
            if bytes[index] == opening {
                depth += 1;
            } else if bytes[index] == closing {
                depth -= 1;
                if depth == 0 {
                    out.push((open + 1, index));
                    from = index;
                    break;
                }
            }
            index += 1;
        }
    }
    out
}

/// The index just past the string literal starting at `start` (a `"`).
fn skip_string(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// The index just past the braced block starting at `start` (a `{`).
fn skip_braces(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0_i32;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index = skip_string(bytes, index);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    bytes.len()
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b':'
}

/// Whether an element can begin at `index` (a `<`): a capitalized name
/// follows, and what precedes it is not an identifier (so `Vec<String>` is
/// not an element).
fn element_starts(bytes: &[u8], index: usize, region_start: usize) -> bool {
    if bytes.get(index) != Some(&b'<') {
        return false;
    }
    let next = bytes.get(index + 1).copied().unwrap_or(b' ');
    if !next.is_ascii_uppercase() && !next.is_ascii_lowercase() {
        return false;
    }
    let previous = bytes[region_start..index].iter().rev().find(|byte| !byte.is_ascii_whitespace());
    let name_end =
        (index + 1..bytes.len()).find(|&at| !is_name_byte(bytes[at])).unwrap_or(bytes.len());
    let capitalized = bytes[index + 1..name_end]
        .rsplit(|byte| *byte == b':')
        .next()
        .and_then(|last| last.first())
        .is_some_and(u8::is_ascii_uppercase);
    capitalized
        && previous.is_none_or(|byte| {
            matches!(byte, b'(' | b'[' | b'{' | b'}' | b',' | b'=' | b'>' | b';' | b'|')
        })
}

/// Every top-level element in `text[start..end]`.
#[must_use]
pub fn elements(text: &str, start: usize, end: usize) -> Vec<ElementRange> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = start;
    while index < end {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index),
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = text[index..end].find('\n').map_or(end, |line| index + line);
            }
            b'<' if element_starts(bytes, index, start) => match element(text, index, end) {
                Some(element) => {
                    index = element.end;
                    out.push(element);
                }
                None => index += 1,
            },
            _ => index += 1,
        }
    }
    out
}

/// The element whose `<` is at `start`.
fn element(text: &str, start: usize, limit: usize) -> Option<ElementRange> {
    let bytes = text.as_bytes();
    let name_end = (start + 1..limit).find(|&at| !is_name_byte(bytes[at]))?;
    let name = text[start + 1..name_end].to_owned();
    let mut attrs = Vec::new();
    let mut index = name_end;
    let (open_end, self_closing) = loop {
        let byte = *bytes.get(index)?;
        if index >= limit {
            return None;
        }
        match byte {
            b'/' if bytes.get(index + 1) == Some(&b'>') => break (index + 2, true),
            b'>' => break (index + 1, false),
            b'{' => index = skip_braces(bytes, index),
            b'"' => index = skip_string(bytes, index),
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let name_start = index;
                while index < limit
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let attr_name = text[name_start..index].to_owned();
                let mut after = index;
                while after < limit && bytes[after].is_ascii_whitespace() {
                    after += 1;
                }
                if bytes.get(after) == Some(&b'=') {
                    let mut value_start = after + 1;
                    while value_start < limit && bytes[value_start].is_ascii_whitespace() {
                        value_start += 1;
                    }
                    let value_end = match bytes.get(value_start) {
                        Some(b'"') => skip_string(bytes, value_start),
                        Some(b'{') => skip_braces(bytes, value_start),
                        _ => (value_start..limit)
                            .find(|&at| {
                                bytes[at].is_ascii_whitespace()
                                    || bytes[at] == b'>'
                                    || bytes[at] == b'/'
                            })
                            .unwrap_or(limit),
                    };
                    attrs.push(AttrRange {
                        name: attr_name,
                        name_start,
                        value: (value_start, value_end),
                    });
                    index = value_end;
                } else {
                    attrs.push(AttrRange { name: attr_name, name_start, value: (index, index) });
                }
            }
            _ => index += 1,
        }
    };
    if self_closing {
        return Some(ElementRange {
            name,
            start,
            open_end,
            end: open_end,
            self_closing,
            attrs,
            children: Vec::new(),
        });
    }
    // The children, up to this element's own closing tag.
    let closing = format!("</{name}>");
    let mut children = Vec::new();
    let mut index = open_end;
    while index < limit {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index),
            b'{' => {
                let block_end = skip_braces(bytes, index);
                children.extend(elements(text, index + 1, block_end.saturating_sub(1)));
                index = block_end;
            }
            b'<' if text[index..].starts_with(&closing) => {
                let end = index + closing.len();
                return Some(ElementRange {
                    name,
                    start,
                    open_end,
                    end,
                    self_closing,
                    attrs,
                    children,
                });
            }
            b'<' if element_starts(bytes, index, open_end)
                || bytes.get(index + 1).is_some_and(u8::is_ascii_uppercase) =>
            {
                let child = element(text, index, limit)?;
                index = child.end;
                children.push(child);
            }
            _ => index += 1,
        }
    }
    None
}

/// Every element in the file, top level first.
#[must_use]
pub fn scan(text: &str, rsx_file: bool) -> Vec<ElementRange> {
    regions(text, rsx_file)
        .into_iter()
        .flat_map(|(start, end)| elements(text, start, end))
        .collect()
}

/// The innermost element containing `offset`.
#[must_use]
pub fn element_at(elements: &[ElementRange], offset: usize) -> Option<&ElementRange> {
    elements.iter().find_map(|element| element.at(offset))
}

/// One replacement: `text[start..end]` becomes `new_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Where it starts.
    pub start: usize,
    /// Where it ends.
    pub end: usize,
    /// What replaces it.
    pub new_text: String,
}

/// Applies `edits` (in any order, not overlapping) to `text`.
#[cfg(test)]
#[must_use]
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut sorted = edits.to_vec();
    sorted.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    let mut out = text.to_owned();
    for edit in sorted {
        out.replace_range(edit.start..edit.end, &edit.new_text);
    }
    out
}

/// The whitespace at the start of the line `offset` is on.
fn indentation(text: &str, offset: usize) -> &str {
    let line_start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let line = &text[line_start..];
    &line[..line.len() - line.trim_start().len()]
}

/// Sets attribute `name` of `element` to `value` (as written:
/// `"Save"`, `{count}`), replacing its value or adding it after the last
/// attribute.
#[must_use]
pub fn set_attribute(text: &str, element: &ElementRange, name: &str, value: &str) -> Edit {
    if let Some(attr) = element.attrs.iter().find(|attr| attr.name == name) {
        if attr.value.0 == attr.value.1 {
            // A flag written without a value: give it one.
            return Edit { start: attr.value.0, end: attr.value.1, new_text: format!("={value}") };
        }
        return Edit { start: attr.value.0, end: attr.value.1, new_text: value.to_owned() };
    }
    let at =
        element.attrs.last().map_or(element.start + 1 + element.name.len(), |attr| attr.value.1);
    // Written on its own line when the element's attributes are.
    let multiline = element
        .attrs
        .last()
        .is_some_and(|attr| text[element.start..attr.name_start].contains('\n'));
    let separator = if multiline {
        let last = element.attrs.last().map_or(element.start, |attr| attr.name_start);
        format!("\n{}", indentation(text, last))
    } else {
        " ".to_owned()
    };
    Edit { start: at, end: at, new_text: format!("{separator}{name}={value}") }
}

/// Removes `element`, and the line it stood on when it stood alone.
#[must_use]
pub fn remove_element(text: &str, element: &ElementRange) -> Edit {
    let line_start = text[..element.start].rfind('\n').map_or(0, |at| at + 1);
    let alone_before = text[line_start..element.start].trim().is_empty();
    let line_end = text[element.end..].find('\n').map_or(text.len(), |at| element.end + at);
    let alone_after = text[element.end..line_end].trim().is_empty();
    if alone_before && alone_after && line_start > 0 {
        // The whole line, and the line break that ended the previous one.
        return Edit { start: line_start - 1, end: line_end, new_text: String::new() };
    }
    Edit { start: element.start, end: element.end, new_text: String::new() }
}

/// Inserts `markup` as child `index` of `parent` (appending past the end),
/// on its own line, indented like its siblings.
///
/// # Errors
///
/// `parent` closes itself, so it has no children to insert among.
pub fn insert_element(
    text: &str,
    parent: &ElementRange,
    index: usize,
    markup: &str,
) -> Result<Edit, String> {
    if parent.self_closing {
        return Err(format!("`<{} />` closes itself: it has no children", parent.name));
    }
    let child_indent = parent.children.first().map_or_else(
        || format!("{}    ", indentation(text, parent.start)),
        |child| indentation(text, child.start).to_owned(),
    );
    let indented = markup.lines().collect::<Vec<_>>().join(&format!("\n{child_indent}"));
    // Before the child it goes before, or before the closing tag.
    let at = parent
        .children
        .get(index)
        .map_or(parent.end - parent.name.len() - 3, |before| before.start);
    let line_start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    Ok(if text[line_start..at].trim().is_empty() {
        Edit { start: line_start, end: line_start, new_text: format!("{child_indent}{indented}\n") }
    } else {
        Edit { start: at, end: at, new_text: indented }
    })
}

/// Moves `element` to be child `index` of `parent`, as two edits.
///
/// # Errors
///
/// As [`insert_element`], or `parent` is inside `element`.
pub fn move_element(
    text: &str,
    element: &ElementRange,
    parent: &ElementRange,
    index: usize,
) -> Result<Vec<Edit>, String> {
    if parent.start >= element.start && parent.end <= element.end {
        return Err("an element cannot move into itself".into());
    }
    let markup = text[element.start..element.end].to_owned();
    // Its own lines lose their old indentation and take the new one.
    let old_indent = indentation(text, element.start);
    let markup: Vec<&str> = markup.lines().collect();
    let markup = markup
        .iter()
        .enumerate()
        .map(
            |(line, text)| {
                if line == 0 { *text } else { text.strip_prefix(old_indent).unwrap_or(text) }
            },
        )
        .collect::<Vec<_>>()
        .join("\n");
    Ok(vec![remove_element(text, element), insert_element(text, parent, index, &markup)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"use rustnative_core::{Node, rsx};

// A comment the edits must keep.
fn view(items: Vec<String>) -> Node {
    rsx! {
        <Column key="root" gap=4>
            <Label key="title" text="Hello" />
            // A comment between children.
            {items.iter().map(|item| <Label key={item.clone()} text={item.clone()} />)}
            <Button
                key="save"
                text="Save"
            />
        </Column>
    }
}
"#;

    fn root() -> ElementRange {
        let elements = scan(FILE, false);
        assert_eq!(elements.len(), 1, "{elements:?}");
        elements[0].clone()
    }

    #[test]
    fn the_scanner_finds_elements_attributes_and_nested_markup_but_not_generics() {
        let root = root();
        assert_eq!(root.name, "Column");
        assert_eq!(
            root.attrs.iter().map(|attr| attr.name.as_str()).collect::<Vec<_>>(),
            ["key", "gap"]
        );
        let names: Vec<&str> = root.children.iter().map(|child| child.name.as_str()).collect();
        assert_eq!(names, ["Label", "Label", "Button"], "the braced map's element is a child too");
        let save = &root.children[2];
        assert_eq!(&FILE[save.attrs[1].value.0..save.attrs[1].value.1], "\"Save\"");
        let offset = FILE.find("Save").unwrap();
        assert_eq!(
            element_at(std::slice::from_ref(&root), offset).map(|element| element.name.as_str()),
            Some("Button")
        );
        // A `.rsx` file is markup throughout, with Rust around it.
        let rsx = "fn f() -> Vec<Node> {\n    vec![<Label key=\"a\" text=\"A\" />]\n}\n";
        let found = scan(rsx, true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Label");
    }

    #[test]
    fn setting_an_attribute_touches_only_its_value_or_adds_it_in_the_element_s_style() {
        let root = root();
        let title = &root.children[0];
        let edited = apply(FILE, &[set_attribute(FILE, title, "text", "\"Hi\"")]);
        assert!(edited.contains(r#"<Label key="title" text="Hi" />"#));
        assert!(edited.contains("// A comment the edits must keep."));
        let added = apply(FILE, &[set_attribute(FILE, title, "opacity", "{0.5}")]);
        assert!(added.contains(r#"<Label key="title" text="Hello" opacity={0.5} />"#), "{added}");
        // Attributes one per line get the new one on its own line.
        let save = &root.children[2];
        let added = apply(FILE, &[set_attribute(FILE, save, "disabled", "{true}")]);
        assert!(
            added.contains(
                "                text=\"Save\"\n                disabled={true}\n            />"
            ),
            "{added}"
        );
    }

    #[test]
    fn removing_inserting_and_moving_keep_everything_else() {
        let root = root();
        let removed = apply(FILE, &[remove_element(FILE, &root.children[0])]);
        assert!(!removed.contains("key=\"title\""));
        assert!(
            removed.contains(
                "        <Column key=\"root\" gap=4>\n            // A comment between children."
            ),
            "{removed}"
        );

        let inserted = apply(
            FILE,
            &[insert_element(FILE, &root, 0, "<Label key=\"new\" text=\"New\" />").unwrap()],
        );
        assert!(inserted.contains("gap=4>\n            <Label key=\"new\" text=\"New\" />\n            <Label key=\"title\""), "{inserted}");
        let appended = apply(
            FILE,
            &[insert_element(FILE, &root, 9, "<Label key=\"end\" text=\"End\" />").unwrap()],
        );
        assert!(
            appended.contains(
                "            />\n            <Label key=\"end\" text=\"End\" />\n        </Column>"
            ),
            "{appended}"
        );
        assert!(insert_element(FILE, &root.children[0], 0, "<Label />").is_err());

        let moved = apply(FILE, &move_element(FILE, &root.children[0], &root, 9).unwrap());
        let title = moved.find("key=\"title\"").unwrap();
        assert!(title > moved.find("key=\"save\"").unwrap(), "moved to the end: {moved}");
        assert_eq!(moved.matches("key=\"title\"").count(), 1);
        assert!(move_element(FILE, &root, &root.children[0], 0).is_err());
    }
}
