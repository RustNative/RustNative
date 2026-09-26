//! `rustnative i18n`: the translator's workflow (`PLAN.md` Milestone 46).
//!
//! It reads both syntaxes the same way: `.rs` files and `.rsx` files, and
//! builder calls and markup (including markup inside `rsx!`) alike.
//!
//! - `extract` finds every message the code uses (`messages::name(…)`,
//!   `Message::new("id")`) and adds the missing ones to the source locale's
//!   catalogue. Each new entry is placed with a context comment naming
//!   where it is used. Messages the code never uses are reported.
//! - `merge` brings every translation up to the source. It adds what a
//!   translation lacks, as the source text marked `# needs-translation`,
//!   and reports what a translation has that the source no longer does.
//! - `show <id>` lists where a message is used and which locales translate
//!   it.
//! - `lint` reports text written as a literal where a message belongs. In
//!   builder calls that is `Node::label(key, "…")` and its siblings; in
//!   markup it is `text="…"`. It exits non-zero when there are any, for CI.
//!   `[i18n] allow` lists literals that stay (a brand name, a symbol).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::error::{Error, Result};
use crate::project::Project;

/// What to do.
#[derive(Debug, Subcommand)]
pub enum I18nCommand {
    /// Add the messages the code uses to the source catalogue.
    Extract,
    /// Bring every translation up to the source catalogue.
    Merge,
    /// Where a message is used, and which locales translate it.
    Show {
        /// The message's identifier.
        id: String,
    },
    /// Report literal text where a message belongs.
    Lint,
}

/// One use of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Use {
    file: PathBuf,
    line: usize,
}

fn sources(root: &Path) -> Vec<PathBuf> {
    fn walk(folder: &Path, into: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, into);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "rs" || extension == "rsx")
            {
                into.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root.join("src"), &mut files);
    files.sort();
    files
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

/// Message identifiers used in `text`: `messages::fn_name(` (the
/// identifier, underscores as the catalogue writes them resolved by the
/// caller) and `Message::new("id")`.
fn uses_in(text: &str) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for (marker, quoted) in [("messages::", false), ("Message::new(\"", true)] {
        let mut from = 0;
        while let Some(at) = text[from..].find(marker) {
            let start = from + at + marker.len();
            let end = start
                + text[start..]
                    .find(
                        |c: char| {
                            if quoted { c == '"' } else { !(c.is_alphanumeric() || c == '_') }
                        },
                    )
                    .unwrap_or(text.len() - start);
            let name = text[start..end].trim_start_matches("r#").to_owned();
            let called = quoted || text[end..].starts_with('(');
            if called && !name.is_empty() && name != "catalogues" {
                found.push((name, line_of(text, start)));
            }
            from = end;
        }
    }
    found
}

/// The catalogue identifier a use names: as written, or with `_` as `-`
/// (a generated function's name) when that is what the catalogue has.
fn identifier(name: &str, known: &BTreeSet<String>) -> String {
    if known.contains(name) {
        return name.to_owned();
    }
    let dashed = name.replace('_', "-");
    if known.contains(&dashed) { dashed } else { name.to_owned() }
}

fn catalogue_path(root: &Path, locale: &str) -> PathBuf {
    root.join("locales").join(format!("{locale}.ftl"))
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).or_else(|cause| {
        if cause.kind() == std::io::ErrorKind::NotFound {
            Ok(String::new())
        } else {
            Err(Error::Io { what: format!("read {}", path.display()), cause })
        }
    })
}

fn parse(path: &Path, text: &str) -> Result<rustnative_i18n::Bundle> {
    rustnative_i18n::parse(text).map_err(|errors| {
        Error::Usage(
            errors
                .iter()
                .map(|error| format!("{}:{error}", path.display()))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })
}

/// Every use of every message, by identifier.
fn all_uses(root: &Path, known: &BTreeSet<String>) -> Result<BTreeMap<String, Vec<Use>>> {
    let mut uses: BTreeMap<String, Vec<Use>> = BTreeMap::new();
    for file in sources(root) {
        let text = read(&file)?;
        for (name, line) in uses_in(&text) {
            let relative = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
            uses.entry(identifier(&name, known)).or_default().push(Use { file: relative, line });
        }
    }
    Ok(uses)
}

/// Message `id`'s entry as written in `text` (its lines, comment excluded).
fn entry_text(text: &str, message: &rustnative_i18n::Message) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = message.line - 1;
    let mut end = start + 1;
    while end < lines.len()
        && !lines[end].trim().is_empty()
        && (lines[end].starts_with(char::is_whitespace) || lines[end].trim() == "}")
    {
        end += 1;
    }
    lines[start..end].join("\n")
}

fn append(path: &Path, text: &str, addition: &str) -> Result<()> {
    let mut out = text.trim_end().to_owned();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(addition.trim_end());
    out.push('\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|cause| Error::Io { what: format!("create {}", parent.display()), cause })?;
    }
    std::fs::write(path, out)
        .map_err(|cause| Error::Io { what: format!("write {}", path.display()), cause })
}

/// The locales with a catalogue.
fn locales(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(root.join("locales"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ftl"))
        .filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .collect();
    found.sort();
    found
}

/// Literal text where a message belongs: `(file, line, text)`.
fn literals(root: &Path, allow: &[String]) -> Result<Vec<(PathBuf, usize, String)>> {
    let worth = |text: &str| {
        text.chars().any(char::is_alphabetic) && !allow.iter().any(|allowed| allowed == text)
    };
    let mut found = Vec::new();
    for file in sources(root) {
        let text = read(&file)?;
        let relative = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
        // Builder calls: the text argument after the key.
        for constructor in [
            "Node::label(",
            "Node::button(",
            "Node::text_input(",
            "Node::label_with_layout(",
            "Node::button_with_layout(",
        ] {
            let mut from = 0;
            while let Some(at) = text[from..].find(constructor) {
                let start = from + at + constructor.len();
                from = start;
                let rest = &text[start..];
                // Skip the key argument: up to the first top-level comma.
                let Some(comma) = rest.find(',') else { continue };
                let after = rest[comma + 1..].trim_start();
                if let Some(literal) = after.strip_prefix('"') {
                    if let Some(end) = literal.find('"') {
                        let value = &literal[..end];
                        if worth(value) {
                            found.push((relative.clone(), line_of(&text, start), value.to_owned()));
                        }
                    }
                }
            }
        }
        // Markup: `text="…"` on an element, in `.rsx` files and `rsx!`.
        let rsx_file = file.extension().is_some_and(|extension| extension == "rsx");
        for (start, end) in crate::markup_edit::regions(&text, rsx_file) {
            let region = &text[start..end];
            let mut from = 0;
            while let Some(at) = region[from..].find("text=\"") {
                let value_start = from + at + "text=\"".len();
                let Some(length) = region[value_start..].find('"') else { break };
                let value = &region[value_start..value_start + length];
                if worth(value) {
                    found.push((
                        relative.clone(),
                        line_of(&text, start + value_start),
                        value.to_owned(),
                    ));
                }
                from = value_start + length;
            }
        }
    }
    Ok(found)
}

/// Runs `command` for the project around `here`.
///
/// # Errors
///
/// A catalogue does not parse, a file cannot be written, or (`lint`)
/// literals were found.
pub fn run(here: &Path, command: &I18nCommand) -> Result<()> {
    let project = Project::find(here)?;
    let root = &project.root;
    let source = project.config.i18n.source.clone();
    let source_path = catalogue_path(root, &source);
    let source_text = read(&source_path)?;
    let bundle = parse(&source_path, &source_text)?;
    let known: BTreeSet<String> = bundle.messages.keys().cloned().collect();
    match command {
        I18nCommand::Extract => {
            let uses = all_uses(root, &known)?;
            let mut added = String::new();
            for (id, places) in uses.iter().filter(|(id, _)| !known.contains(*id)) {
                let first = &places[0];
                let _ = write!(
                    added,
                    "# Used in {}:{}. Write the text.\n{id} = {id}\n\n",
                    first.file.display(),
                    first.line
                );
                println!("extract: added `{id}` to {}", source_path.display());
            }
            if !added.is_empty() {
                append(&source_path, &source_text, &added)?;
            }
            for id in known.iter().filter(|id| !uses.contains_key(*id)) {
                println!("extract: `{id}` is never used");
            }
            Ok(())
        }
        I18nCommand::Merge => {
            for locale in locales(root).into_iter().filter(|locale| *locale != source) {
                let path = catalogue_path(root, &locale);
                let text = read(&path)?;
                let translation = parse(&path, &text)?;
                let mut added = String::new();
                for (id, message) in
                    bundle.messages.iter().filter(|(id, _)| !translation.messages.contains_key(*id))
                {
                    let _ = write!(
                        added,
                        "# needs-translation\n{}\n\n",
                        entry_text(&source_text, message)
                    );
                    println!("merge: {locale} needs `{id}`");
                }
                if !added.is_empty() {
                    append(&path, &text, &added)?;
                }
                for (id, message) in
                    translation.messages.iter().filter(|(id, _)| !known.contains(*id))
                {
                    println!(
                        "merge: {locale}.ftl:{}: `{id}` is no longer in {source}.ftl",
                        message.line
                    );
                }
            }
            Ok(())
        }
        I18nCommand::Show { id } => {
            let uses = all_uses(root, &known)?;
            match uses.get(id) {
                Some(places) => {
                    for place in places {
                        println!("{}:{}", place.file.display(), place.line);
                    }
                }
                None => println!("`{id}` is not used in src/"),
            }
            for locale in locales(root) {
                let path = catalogue_path(root, &locale);
                let translation = parse(&path, &read(&path)?)?;
                let state =
                    if translation.messages.contains_key(id) { "translated" } else { "missing" };
                println!("{locale}: {state}");
            }
            Ok(())
        }
        I18nCommand::Lint => {
            let found = literals(root, &project.config.i18n.allow)?;
            for (file, line, text) in &found {
                println!(
                    "{}:{line}: \"{text}\" is written as a literal; show a message instead",
                    file.display()
                );
            }
            if found.is_empty() {
                println!("lint: no untranslated literals");
                Ok(())
            } else {
                Err(Error::Usage(format!("{} untranslated literal(s)", found.len())))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_are_found_in_both_spellings_of_a_message() {
        let text = "let a = messages::inbox_count(3);\nlet b = Message::new(\"hello-world\");\nlet c = messages::catalogues();\n";
        let found = uses_in(text);
        assert_eq!(found, [("inbox_count".to_owned(), 1), ("hello-world".to_owned(), 2)]);
        let known = BTreeSet::from(["inbox-count".to_owned()]);
        assert_eq!(identifier("inbox_count", &known), "inbox-count");
        assert_eq!(identifier("unknown_one", &known), "unknown_one");
    }

    #[test]
    fn an_entry_is_copied_with_its_selector() {
        let text = "# c\ninbox = { $n ->\n    [one] One\n   *[other] Many\n}\nnext = Next\n";
        let bundle = rustnative_i18n::parse(text).unwrap();
        assert_eq!(
            entry_text(text, &bundle.messages["inbox"]),
            "inbox = { $n ->\n    [one] One\n   *[other] Many\n}"
        );
        assert_eq!(entry_text(text, &bundle.messages["next"]), "next = Next");
    }
}
