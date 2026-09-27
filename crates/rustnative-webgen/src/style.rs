//! `classes!` and `styles!` in client logic: compiled here, at build time,
//! against the project's style file, into the set descriptors the runtime's
//! realizer takes — the same class names and rules the server's realizer
//! writes for the same class string.

use std::path::PathBuf;

use rustnative_style::{DeclarationSet, SetKind, Vocabulary};

fn style_file() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let configured = std::fs::read_to_string(root.join("rustnative.toml")).ok().and_then(|text| {
        let mut in_style = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_style = line == "[style]";
            } else if in_style {
                if let Some(value) =
                    line.strip_prefix("file").map(str::trim).and_then(|rest| rest.strip_prefix('='))
                {
                    return Some(value.trim().trim_matches('"').to_owned());
                }
            }
        }
        None
    });
    let path = root.join(configured.as_deref().unwrap_or("app.css"));
    path.is_file().then_some(path)
}

fn vocabulary() -> Result<Vocabulary, String> {
    match style_file() {
        None => Ok(Vocabulary::defaults()),
        Some(path) => {
            let source = std::fs::read_to_string(&path)
                .map_err(|error| format!("reading {}: {error}", path.display()))?;
            Vocabulary::with_style_file(&source).map_err(|errors| {
                errors.iter().map(|error| error.message.clone()).collect::<Vec<_>>().join("\n")
            })
        }
    }
}

/// A compiled class string or declaration block.
pub struct Compiled {
    /// Its descriptor, as a JavaScript (JSON) literal.
    pub descriptor: String,
    /// Its class and rule, when it has context-free rules.
    pub rule: Option<(String, String)>,
    /// The tokens it refers to.
    pub tokens: Vec<String>,
}

/// Compiles `text` as classes (`declarations: false`) or declarations.
pub fn compile(text: &str, declarations: bool) -> Result<Compiled, String> {
    let vocabulary = vocabulary()?;
    let resolved = if declarations {
        vocabulary.resolve_declarations(text)
    } else {
        vocabulary.resolve_classes(text)
    };
    let resolved = resolved.map_err(|problems| {
        problems.iter().map(|problem| problem.message.clone()).collect::<Vec<_>>().join("; ")
    })?;
    // A macro runs once per expansion; the set lives for the compilation.
    let leaked: &'static [rustnative_style::ConditionalDeclaration] =
        Box::leak(resolved.into_boxed_slice());
    let set = DeclarationSet::from_sources(
        leaked,
        &[],
        if declarations { SetKind::Declarations } else { SetKind::Classes },
    );
    let descriptor = rustnative_style::web::set_descriptor(set);
    let rule = match (descriptor["d"].as_str(), rustnative_style::web::declaration_rules(set)) {
        (Some(class), Some(rules)) => Some((
            class.to_owned(),
            format!("@layer rn-d{{{}}}", rules.replace('&', &format!(".{class}"))),
        )),
        _ => None,
    };
    let tokens = descriptor["tokens"]
        .as_array()
        .map(|tokens| {
            tokens.iter().filter_map(|token| token.as_str().map(ToOwned::to_owned)).collect()
        })
        .unwrap_or_default();
    Ok(Compiled { descriptor: descriptor.to_string(), rule, tokens })
}
