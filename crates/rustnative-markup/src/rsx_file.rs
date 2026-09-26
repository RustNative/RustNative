//! The `.rsx` compiler: markup as a plain expression in a Rust file.
//!
//! A `.rsx` file is Rust with one more kind of expression — an element —
//! written wherever an expression is valid, with no wrapper. The host
//! compiler does not accept it, so this compiler lowers the file first, and
//! it does exactly one thing: it finds each markup expression and wraps it
//! in `::rustnative_core::rsx!(…)`, inserting the enclosing function's
//! component context where a component element needs one. Every other byte
//! is emitted where it was, lines never move, and parsing, lowering, and
//! diagnostics stay in the macro — so a `.rsx` file cannot accept anything
//! `rsx!` rejects (`PLAN.md` 2.9).
//!
//! # The disambiguation rule
//!
//! In expression-start position — at the start of a group, or after one of
//! `= ( [ { , ; => ! & * | || && + - / % ^ == != <= >= < > :` or the
//! keywords `return`, `break`, `in` — a `<` followed by an identifier or by
//! `>` begins an element. A qualified path (`<T>::item`, `<T as Trait>::f`)
//! is recognized by the `::` after its closing `>` or the `as` inside it,
//! and stays Rust. `<` after anything else is a comparison or generics. The
//! file is tokenized by the host language's own lexer, so the rule is
//! applied to exactly the tokens `rustc` will see.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, Spacing, Span, TokenStream, TokenTree};
use serde::{Deserialize, Serialize};

/// Where a `.rsx` file lives, so its `mod` declarations can be resolved.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// The `.rsx` file.
    pub source_path: PathBuf,
    /// The crate's `src` directory; lowered files mirror paths under it.
    pub src_root: PathBuf,
    /// The macro path each markup expression is wrapped in; `None` for
    /// `::rustnative_core::rsx!`. The formatter uses a private marker, so it
    /// can tell the wrappers it must remove from `rsx!` calls a developer
    /// wrote.
    pub wrapper: Option<String>,
}

/// A compiled `.rsx` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsxOutput {
    /// The lowered Rust source.
    pub code: String,
    /// How to map positions in `code` back to the `.rsx` file.
    pub map: SourceMap,
}

/// A problem found while compiling a `.rsx` file, at a 1-based position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RsxError {
    /// The line.
    pub line: usize,
    /// The column, in characters.
    pub column: usize,
    /// What is wrong.
    pub message: String,
}

impl std::fmt::Display for RsxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

/// Columns inserted on each line of a lowered file: `(lowered column, how
/// many characters were inserted there)`, 1-based. Lines themselves never
/// move, so this is the whole mapping.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMap {
    /// The `.rsx` file this was lowered from.
    pub source: String,
    /// Per line, the insertions on it.
    pub lines: BTreeMap<usize, Vec<(usize, isize)>>,
}

impl SourceMap {
    /// The `.rsx` position of `(line, column)` in the lowered file. A
    /// column inside inserted text maps to where the insertion was made.
    #[must_use]
    pub fn to_source(&self, line: usize, column: usize) -> (usize, usize) {
        let Some(edits) = self.lines.get(&line) else { return (line, column) };
        let mut shift: isize = 0;
        for (at, inserted) in edits {
            if column < *at {
                break;
            }
            let end = at.saturating_add_signed(*inserted);
            if *inserted > 0 && column < end {
                return (line, at.saturating_add_signed(-shift));
            }
            shift += inserted;
        }
        (line, column.saturating_add_signed(-shift))
    }

    /// The lowered position of `(line, column)` in the `.rsx` file.
    #[must_use]
    pub fn to_lowered(&self, line: usize, column: usize) -> (usize, usize) {
        let Some(edits) = self.lines.get(&line) else { return (line, column) };
        let mut shift: isize = 0;
        for (at, inserted) in edits {
            let source_at = at.saturating_add_signed(-shift);
            if column < source_at {
                break;
            }
            shift += inserted;
        }
        (line, column.saturating_add_signed(shift))
    }

    /// Serializes the map (written beside the lowered file).
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Reads a map written by [`Self::to_json`].
    #[must_use]
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

/// What precedes a token, for the disambiguation rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prev {
    Start,
    ExprStart,
    Other,
}

#[derive(Debug)]
struct Site {
    start: usize,
    end: usize,
    context: Option<String>,
}

#[derive(Debug)]
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

struct Scanner<'a> {
    options: &'a CompileOptions,
    source: &'a str,
    sites: Vec<Site>,
    edits: Vec<Edit>,
    errors: Vec<RsxError>,
}

/// The function context a component element in this group may use.
#[derive(Debug, Clone)]
enum FnContext {
    None,
    One(String),
    Ambiguous(Vec<String>),
}

/// Compiles one `.rsx` file.
///
/// # Errors
///
/// Every problem found: a markup expression that does not parse, a
/// component element with no (or more than one) `ComponentContext`
/// parameter to use, an inner attribute (which `include!` cannot carry),
/// or a file the host lexer rejects.
pub fn compile(source: &str, options: &CompileOptions) -> Result<RsxOutput, Vec<RsxError>> {
    let tokens = TokenStream::from_str(source).map_err(|error| {
        let start = error.span().start();
        vec![RsxError { line: start.line, column: start.column + 1, message: error.to_string() }]
    })?;
    let mut scanner =
        Scanner { options, source, sites: Vec::new(), edits: Vec::new(), errors: Vec::new() };
    scanner.scan(tokens, &FnContext::None);
    scanner.inner_doc_comments();
    if !scanner.errors.is_empty() {
        return Err(scanner.errors);
    }
    let mut edits: Vec<Edit> = scanner.edits;
    let wrapper = options.wrapper.as_deref().unwrap_or("::rustnative_core::rsx!");
    for site in scanner.sites {
        let prefix = site
            .context
            .map_or_else(|| format!("{wrapper}("), |context| format!("{wrapper}(in {context}, "));
        edits.push(Edit { start: site.start, end: site.start, replacement: prefix });
        edits.push(Edit { start: site.end, end: site.end, replacement: ")".to_owned() });
    }
    edits.sort_by_key(|edit| (edit.start, edit.end));
    Ok(apply_edits(source, &edits, &options.source_path))
}

fn apply_edits(source: &str, edits: &[Edit], path: &Path) -> RsxOutput {
    let mut code = String::with_capacity(source.len() + edits.len() * 24);
    let mut map = SourceMap { source: path.display().to_string(), lines: BTreeMap::new() };
    let mut cursor = 0;
    for edit in edits {
        code.push_str(&source[cursor..edit.start]);
        // The position of the edit in the *lowered* file.
        let line = code.matches('\n').count() + 1;
        let line_start = code.rfind('\n').map_or(0, |index| index + 1);
        let column = code[line_start..].chars().count() + 1;
        let removed = source[edit.start..edit.end].chars().count();
        let inserted = edit.replacement.chars().count();
        code.push_str(&edit.replacement);
        #[allow(clippy::cast_possible_wrap, reason = "line lengths are far below isize::MAX")]
        let delta = inserted as isize - removed as isize;
        if delta != 0 {
            map.lines.entry(line).or_default().push((column, delta));
        }
        cursor = edit.end;
    }
    code.push_str(&source[cursor..]);
    RsxOutput { code, map }
}

fn position(span: Span) -> (usize, usize) {
    let start = span.start();
    (start.line, start.column + 1)
}

impl Scanner<'_> {
    fn error(&mut self, span: Span, message: impl Into<String>) {
        let (line, column) = position(span);
        self.errors.push(RsxError { line, column, message: message.into() });
    }

    fn scan(&mut self, stream: TokenStream, context: &FnContext) {
        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        let mut prev = Prev::Start;
        let mut pending_fn: Option<FnContext> = None;
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            if let TokenTree::Punct(punct) = token {
                if punct.as_char() == '<' && prev != Prev::Other && starts_element(&tokens[index..])
                {
                    match crate::rsx_scan::match_element(&tokens[index..]) {
                        Ok(element) => {
                            let consumed = element.len;
                            let site_context = if element.has_component {
                                match context {
                                    FnContext::One(name) => Some(name.clone()),
                                    FnContext::None => {
                                        self.error(
                                            element.open,
                                            "a component element needs a `ComponentContext`, and the enclosing \
                                             function has no such parameter; write `rsx!(in context, …)` instead",
                                        );
                                        None
                                    }
                                    FnContext::Ambiguous(names) => {
                                        self.error(
                                            element.open,
                                            format!(
                                                "the enclosing function has more than one `ComponentContext` \
                                                 parameter ({}); name one with `rsx!(in context, …)`",
                                                names.join(", ")
                                            ),
                                        );
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            self.sites.push(Site {
                                start: element.open.byte_range().start,
                                end: element.close.byte_range().end,
                                context: site_context,
                            });
                            // The Rust inside the element — attribute values,
                            // braced children, conditions — is `.rsx` Rust
                            // too, and may hold markup of its own.
                            for stream in element.rust {
                                self.scan(stream, context);
                            }
                            index += consumed.max(1);
                            prev = Prev::Other;
                            continue;
                        }
                        Err(error) => {
                            self.error(error.span, error.message);
                            return;
                        }
                    }
                }
            }
            match token {
                TokenTree::Group(group) => {
                    let inner_context = match (group.delimiter(), pending_fn.take()) {
                        (Delimiter::Brace, Some(fn_context)) => fn_context,
                        (Delimiter::Parenthesis, Some(fn_context)) => {
                            // A tuple-returning signature's parameters are
                            // not the body; keep waiting for the brace.
                            pending_fn = Some(fn_context);
                            context.clone()
                        }
                        (_, other) => {
                            pending_fn = other;
                            context.clone()
                        }
                    };
                    self.scan(group.stream(), &inner_context);
                    prev = Prev::Other;
                }
                TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    if name == "fn" {
                        pending_fn = Some(fn_context_from(&tokens[index + 1..]));
                    } else if name == "mod" {
                        self.module_declaration(&tokens[index..]);
                    }
                    prev = if matches!(name.as_str(), "return" | "break" | "in") {
                        Prev::ExprStart
                    } else {
                        Prev::Other
                    };
                    if name == "fn" || name == "impl" || name == "where" {
                        prev = Prev::Other;
                    }
                }
                TokenTree::Punct(punct) => {
                    if punct.as_char() == ';' {
                        // A bodiless signature (`fn f();`) ends here.
                        pending_fn = None;
                    }
                    prev = classify_punct(
                        punct.as_char(),
                        punct.spacing(),
                        tokens.get(index + 1),
                        index.checked_sub(1).and_then(|previous| tokens.get(previous)),
                    );
                }
                TokenTree::Literal(_) => prev = Prev::Other,
            }
            index += 1;
        }
    }

    /// Rewrites `mod name;` so a `.rsx` module can have `.rs` and `.rsx`
    /// children alike (the lowered file lives in `OUT_DIR`, where a plain
    /// `mod name;` would look in the wrong directory).
    fn module_declaration(&mut self, tokens: &[TokenTree]) {
        let (Some(TokenTree::Ident(name)), Some(TokenTree::Punct(semi))) =
            (tokens.get(1), tokens.get(2))
        else {
            return;
        };
        if semi.as_char() != ';' {
            return;
        }
        let Some(TokenTree::Ident(keyword)) = tokens.first() else { return };
        let module = name.to_string();
        let base = module_base(&self.options.source_path);
        let candidates = [
            (base.join(format!("{module}.rs")), false),
            (base.join(&module).join("mod.rs"), false),
            (base.join(format!("{module}.rsx")), true),
            (base.join(&module).join("mod.rsx"), true),
        ];
        let Some((path, is_rsx)) = candidates.into_iter().find(|(path, _)| path.exists()) else {
            return;
        };
        let replacement = if is_rsx {
            let relative =
                path.strip_prefix(&self.options.src_root).unwrap_or(&path).with_extension("rs");
            let relative = relative.to_string_lossy().replace('\\', "/");
            format!("mod {module} {{ include!(concat!(env!(\"OUT_DIR\"), \"/rsx/{relative}\")); }}")
        } else {
            let absolute = path.canonicalize().unwrap_or(path).to_string_lossy().replace('\\', "/");
            let absolute = absolute.trim_start_matches("//?/").to_owned();
            format!("#[path = \"{absolute}\"] mod {module};")
        };
        self.edits.push(Edit {
            start: keyword.span().byte_range().start,
            end: semi.span().byte_range().end,
            replacement,
        });
    }

    /// `include!` cannot carry inner attributes, so inner doc comments
    /// become ordinary comments (same length) and any other inner attribute
    /// is an error naming the fix.
    fn inner_doc_comments(&mut self) {
        let mut offset = 0;
        for line in self.source.split_inclusive('\n') {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if trimmed.starts_with("//!") {
                self.edits.push(Edit {
                    start: offset + indent,
                    end: offset + indent + 3,
                    replacement: "// ".to_owned(),
                });
            } else if trimmed.starts_with("#![") {
                let line_number = self.source[..offset].matches('\n').count() + 1;
                self.errors.push(RsxError {
                    line: line_number,
                    column: indent + 1,
                    message: "a `.rsx` module is included into its parent, which cannot carry inner attributes; \
                              put this attribute on the `rsx_mod!` declaration instead"
                        .to_owned(),
                });
            }
            offset += line.len();
        }
    }
}

/// The directory a file's child modules live in.
fn module_base(path: &Path) -> PathBuf {
    let parent = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    match path.file_stem().and_then(|stem| stem.to_str()) {
        Some("mod" | "lib" | "main") | None => parent,
        Some(stem) => parent.join(stem),
    }
}

/// Whether the tokens at a `<` begin an element: `<` then an identifier or
/// `>`, and not a qualified path.
fn starts_element(tokens: &[TokenTree]) -> bool {
    match tokens.get(1) {
        Some(TokenTree::Ident(_)) => !is_qualified_path(tokens),
        Some(TokenTree::Punct(punct)) => punct.as_char() == '>',
        _ => false,
    }
}

/// `<T>::item` or `<T as Trait>::item`: the `<` is matched by a `>` that is
/// followed by `::`, or has `as` at its own depth.
fn is_qualified_path(tokens: &[TokenTree]) -> bool {
    let mut depth = 0_i32;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Punct(punct) if punct.as_char() == '<' => depth += 1,
            TokenTree::Punct(punct) if punct.as_char() == '>' => {
                depth -= 1;
                if depth == 0 {
                    return matches!(
                        (tokens.get(index + 1), tokens.get(index + 2)),
                        (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
                            if a.as_char() == ':' && b.as_char() == ':'
                    );
                }
            }
            TokenTree::Ident(ident) if depth == 1 && ident == "as" => return true,
            // An element's attributes contain `=` at depth one; a qualified
            // path never does.
            TokenTree::Punct(punct)
                if depth == 1 && (punct.as_char() == '=' || punct.as_char() == '/') =>
            {
                return false;
            }
            _ => {}
        }
        if index > 64 {
            return false;
        }
    }
    false
}

fn classify_punct(
    character: char,
    spacing: Spacing,
    next: Option<&TokenTree>,
    previous: Option<&TokenTree>,
) -> Prev {
    // `::` is a path separator, never an expression start.
    let part_of_path = character == ':'
        && (spacing == Spacing::Joint
            || matches!(previous, Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Joint));
    if part_of_path {
        return Prev::Other;
    }
    // `->` (a return type) is not an expression start either.
    if character == '>'
        && matches!(previous, Some(TokenTree::Punct(p)) if p.as_char() == '-' && p.spacing() == Spacing::Joint)
    {
        return Prev::Other;
    }
    if character == '-'
        && spacing == Spacing::Joint
        && matches!(next, Some(TokenTree::Punct(p)) if p.as_char() == '>')
    {
        return Prev::Other;
    }
    if matches!(
        character,
        '=' | ',' | ';' | '!' | '&' | '*' | '|' | '+' | '-' | '/' | '%' | '^' | '<' | '>' | ':'
    ) {
        Prev::ExprStart
    } else {
        Prev::Other
    }
}

/// The `ComponentContext` parameter of the function whose signature starts
/// at `tokens` (just after `fn`).
fn fn_context_from(tokens: &[TokenTree]) -> FnContext {
    let Some(params) = tokens.iter().find_map(|token| match token {
        TokenTree::Group(group) if group.delimiter() == Delimiter::Parenthesis => {
            Some(group.stream())
        }
        _ => None,
    }) else {
        return FnContext::None;
    };
    let tokens: Vec<TokenTree> = params.into_iter().collect();
    let mut names = Vec::new();
    for param in tokens.split(|token| matches!(token, TokenTree::Punct(p) if p.as_char() == ',')) {
        let colon = param.iter().enumerate().position(|(index, token)| {
            matches!(token, TokenTree::Punct(p) if p.as_char() == ':' && p.spacing() == Spacing::Alone)
                && !matches!(index.checked_sub(1).and_then(|i| param.get(i)), Some(TokenTree::Punct(q)) if q.as_char() == ':')
        });
        let Some(colon) = colon else { continue };
        let is_context = param[colon + 1..]
            .iter()
            .any(|token| matches!(token, TokenTree::Ident(ident) if ident == "ComponentContext"));
        if !is_context {
            continue;
        }
        if let Some(TokenTree::Ident(name)) = param[..colon]
            .iter()
            .rev()
            .find(|token| matches!(token, TokenTree::Ident(ident) if ident != "mut"))
        {
            names.push(name.to_string());
        }
    }
    match names.len() {
        0 => FnContext::None,
        1 => FnContext::One(names.remove(0)),
        _ => FnContext::Ambiguous(names),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> CompileOptions {
        CompileOptions {
            source_path: PathBuf::from("src/screen.rsx"),
            src_root: PathBuf::from("src"),
            wrapper: None,
        }
    }

    fn lowered(source: &str) -> String {
        compile(source, &options())
            .map_or_else(|errors| format!("ERR {errors:?}"), |output| output.code)
    }

    #[test]
    fn markup_in_expression_position_is_wrapped_and_nothing_else_changes() {
        let source =
            "fn view() -> Node {\n    let x = <Label key=\"a\" text=\"hi\" />;\n    x\n}\n";
        let code = lowered(source);
        assert_eq!(
            code,
            "fn view() -> Node {\n    let x = ::rustnative_core::rsx!(<Label key=\"a\" text=\"hi\" />);\n    x\n}\n"
        );
    }

    #[test]
    fn comparisons_generics_and_qualified_paths_stay_rust() {
        for source in [
            "fn f(a: i32, b: i32) -> bool { a < b }\n",
            "fn f() -> Vec<u8> { Vec::<u8>::new() }\n",
            "fn f() -> u8 { <u8 as Default>::default() }\n",
            "fn f() -> u8 { <u8>::default() }\n",
            "fn f<T: Default>() -> T { T::default() }\n",
            "fn f() -> Option<Vec<u8>> { None }\n",
            "impl<T> S<T> { fn g(&self) -> bool { 1 < 2 } }\n",
            "fn f(x: &[u8]) -> bool { x.len() <= 3 }\n",
        ] {
            assert_eq!(lowered(source), source, "unchanged: {source}");
        }
    }

    #[test]
    fn markup_is_found_in_every_expression_position() {
        let cases = [
            "fn f() -> Node { <Label key=\"a\" text=\"x\" /> }",
            "fn f() -> Node { return <Label key=\"a\" text=\"x\" />; }",
            "fn f(b: bool) -> Node { if b { <Label key=\"a\" text=\"x\" /> } else { <Label key=\"b\" text=\"y\" /> } }",
            "fn f(v: u8) -> Node { match v { 0 => <Label key=\"a\" text=\"x\" />, _ => <Label key=\"b\" text=\"y\" /> } }",
            "fn f() -> Vec<Node> { vec![<Label key=\"a\" text=\"x\" />, <Label key=\"b\" text=\"y\" />] }",
            "fn f() -> Node { g(<Label key=\"a\" text=\"x\" />) }",
            "fn f() -> impl Fn() -> Node { || <Label key=\"a\" text=\"x\" /> }",
        ];
        for source in cases {
            let code = lowered(source);
            let count = code.matches("::rustnative_core::rsx!(").count();
            let expected = source.matches("<Label").count();
            assert_eq!(count, expected, "{source}\n=> {code}");
        }
    }

    #[test]
    fn the_context_is_found_from_the_enclosing_function() {
        let source = "fn render(&mut self, context: &mut ComponentContext<'_, Msg>) -> Node {\n    <Column key=\"r\"><Screen key=\"s\" /></Column>\n}\n";
        let code = lowered(source);
        assert!(code.contains("::rustnative_core::rsx!(in context, <Column"), "{code}");
    }

    #[test]
    fn a_component_element_without_a_context_is_an_error_at_the_element() {
        let source = "fn view() -> Node {\n    <Screen key=\"s\" />\n}\n";
        let errors = compile(source, &options()).expect_err("no context");
        assert_eq!((errors[0].line, errors[0].column), (2, 5));
        assert!(errors[0].message.contains("no such parameter"));

        let two = "fn f(a: &mut ComponentContext<'_, ()>, b: &mut ComponentContext<'_, ()>) -> Node { <Screen key=\"s\" /> }";
        let errors = compile(two, &options()).expect_err("ambiguous");
        assert!(errors[0].message.contains("more than one"), "{errors:?}");
    }

    #[test]
    fn source_maps_round_trip_every_column_outside_insertions() {
        let source = "fn f() -> Node { let x = <Label key=\"a\" text=\"b\" />; x }\n";
        let output = compile(source, &options()).expect("compiles");
        let map = &output.map;
        // The `<` of the element, and something after the element.
        let source_lt = source.find('<').expect("<") + 1;
        let (line, lowered_lt) = map.to_lowered(1, source_lt);
        assert_eq!(map.to_source(line, lowered_lt), (1, source_lt));
        let source_x = source.rfind('x').expect("x") + 1;
        let (_, lowered_x) = map.to_lowered(1, source_x);
        assert_eq!(&output.code[lowered_x - 1..lowered_x], "x");
        assert_eq!(map.to_source(1, lowered_x), (1, source_x));
        assert_eq!(SourceMap::from_json(&map.to_json()).as_ref(), Some(map));
    }

    #[test]
    fn inner_doc_comments_become_ordinary_comments_of_the_same_length() {
        let source = "//! A screen.\nfn f() -> u8 { 1 }\n";
        let code = lowered(source);
        assert_eq!(code, "//  A screen.\nfn f() -> u8 { 1 }\n");
    }

    #[test]
    fn a_malformed_element_is_reported_where_it_is() {
        let source = "fn f() -> Node {\n    <Column key=\"a\">\n}\n";
        let errors = compile(source, &options()).expect_err("unclosed");
        assert_eq!(errors[0].line, 2, "{errors:?}");
    }
}
