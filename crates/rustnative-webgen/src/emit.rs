//! Writing JavaScript: lines, indentation, and for each line the Rust span it
//! came from (the source map's input).

use proc_macro2::Span;

#[derive(Debug, Default)]
pub struct Writer {
    lines: Vec<(String, Option<Span>)>,
    indent: usize,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes `text` (which may hold several lines) at the current
    /// indentation, attributed to `span`.
    pub fn line(&mut self, text: &str, span: Option<Span>) {
        for part in text.split('\n') {
            let trimmed = part.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            self.lines.push((format!("{}{trimmed}", "  ".repeat(self.indent)), span));
        }
    }

    pub fn open(&mut self, text: &str, span: Option<Span>) {
        self.line(text, span);
        self.indent += 1;
    }

    pub fn close(&mut self, text: &str) {
        self.indent = self.indent.saturating_sub(1);
        self.line(text, None);
    }

    pub fn finish(self) -> (String, Vec<Option<Span>>) {
        let mut text = String::new();
        let mut spans = Vec::with_capacity(self.lines.len());
        for (line, span) in self.lines {
            text.push_str(&line);
            text.push('\n');
            spans.push(span);
        }
        (text, spans)
    }
}

/// A JavaScript string literal for `text`.
pub fn string(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

const RESERVED: &[&str] = &[
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "rn",
    "fns",
    "NaN",
    "Infinity",
    "Object",
    "Math",
    "Number",
    "String",
    "Array",
    "JSON",
];

/// A Rust identifier as a JavaScript one.
pub fn ident(name: &str) -> String {
    let name = name.trim_start_matches("r#");
    if RESERVED.contains(&name) { format!("{name}_") } else { name.to_owned() }
}
