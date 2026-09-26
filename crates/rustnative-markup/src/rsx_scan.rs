//! The `.rsx` compiler's structural matcher: where an element ends, which
//! of its tokens are Rust, and whether it contains a component element.
//!
//! This deliberately does *not* parse the element — the macro does that,
//! once, for both carriers. It only has to find the element's extent and
//! its Rust sub-streams (attribute values, braced children, conditions,
//! scrutinees, iterated expressions), because in a `.rsx` file those are
//! `.rsx` Rust too and may contain markup of their own, which must be
//! wrapped before `rustc` (or `syn`) sees them.

use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};

use crate::table::is_builtin;

/// What the matcher learned about one element.
#[derive(Debug)]
pub(crate) struct Matched {
    /// How many token trees the element occupies.
    pub(crate) len: usize,
    /// The opening `<`.
    pub(crate) open: Span,
    /// The final `>`.
    pub(crate) close: Span,
    /// Every Rust sub-stream inside it.
    pub(crate) rust: Vec<TokenStream>,
    /// Whether it (or a descendant) is a component element.
    pub(crate) has_component: bool,
}

/// A structural problem, at a span.
#[derive(Debug)]
pub(crate) struct MatchError {
    pub(crate) span: Span,
    pub(crate) message: String,
}

type Result<T> = std::result::Result<T, MatchError>;

fn punct(token: Option<&TokenTree>, character: char) -> bool {
    matches!(token, Some(TokenTree::Punct(p)) if p.as_char() == character)
}

fn ident(token: Option<&TokenTree>) -> Option<&proc_macro2::Ident> {
    match token {
        Some(TokenTree::Ident(ident)) => Some(ident),
        _ => None,
    }
}

fn brace(token: Option<&TokenTree>) -> Option<&proc_macro2::Group> {
    match token {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => Some(group),
        _ => None,
    }
}

struct State {
    rust: Vec<TokenStream>,
    has_component: bool,
}

/// Matches the element starting at `tokens[0]` (a `<`).
pub(crate) fn match_element(tokens: &[TokenTree]) -> Result<Matched> {
    let mut state = State { rust: Vec::new(), has_component: false };
    let (len, close) = element(tokens, &mut state)?;
    let open = tokens[0].span();
    Ok(Matched { len, open, close, rust: state.rust, has_component: state.has_component })
}

fn name_at(tokens: &[TokenTree], mut index: usize) -> Result<(String, usize)> {
    let Some(first) = ident(tokens.get(index)) else {
        return Err(MatchError {
            span: tokens[index.min(tokens.len() - 1)].span(),
            message: "expected an element name".into(),
        });
    };
    let mut name = first.to_string();
    index += 1;
    while punct(tokens.get(index), ':') && punct(tokens.get(index + 1), ':') {
        let Some(next) = ident(tokens.get(index + 2)) else { break };
        name.push_str("::");
        name.push_str(&next.to_string());
        index += 3;
    }
    Ok((name, index))
}

/// Returns (tokens consumed, span of the final `>`).
fn element(tokens: &[TokenTree], state: &mut State) -> Result<(usize, Span)> {
    let open = tokens[0].span();
    let (name, mut index) = name_at(tokens, 1)?;
    if !is_builtin(&name) {
        state.has_component = true;
    }
    let unclosed = || MatchError { span: open, message: format!("`<{name}>` is never closed") };
    loop {
        match tokens.get(index) {
            None => return Err(unclosed()),
            Some(TokenTree::Punct(p))
                if p.as_char() == '/' && punct(tokens.get(index + 1), '>') =>
            {
                return Ok((index + 2, tokens[index + 1].span()));
            }
            Some(TokenTree::Punct(p)) if p.as_char() == '>' => {
                index += 1;
                break;
            }
            Some(TokenTree::Punct(p))
                if p.as_char() == '.' && punct(tokens.get(index + 1), '.') =>
            {
                index += 2;
                if let Some(group) = brace(tokens.get(index)) {
                    state.rust.push(group.stream());
                    index += 1;
                } else {
                    // `..path::to::modifier`
                    while ident(tokens.get(index)).is_some()
                        || (punct(tokens.get(index), ':') && punct(tokens.get(index + 1), ':'))
                    {
                        index += if ident(tokens.get(index)).is_some() { 1 } else { 2 };
                    }
                }
            }
            Some(TokenTree::Ident(_)) => {
                index += 1;
                if punct(tokens.get(index), '=') {
                    index += 1;
                    match tokens.get(index) {
                        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => {
                            state.rust.push(group.stream());
                            index += 1;
                        }
                        Some(TokenTree::Literal(_)) => index += 1,
                        // `checked=true`: a Boolean literal is an identifier
                        // to the tokenizer.
                        Some(TokenTree::Ident(word)) if word == "true" || word == "false" => {
                            index += 1;
                        }
                        Some(TokenTree::Punct(p)) if p.as_char() == '-' => index += 2,
                        other => {
                            return Err(MatchError {
                                span: other.map_or(open, TokenTree::span),
                                message: "expected a literal or `{expression}` after `=`".into(),
                            });
                        }
                    }
                }
            }
            Some(other) => {
                return Err(MatchError {
                    span: other.span(),
                    message: "expected an attribute, `/>`, or `>`".into(),
                });
            }
        }
    }
    // Children, up to `</name>`.
    loop {
        if index >= tokens.len() {
            return Err(unclosed());
        }
        if punct(tokens.get(index), '<') && punct(tokens.get(index + 1), '/') {
            let (closing, after) = name_at(tokens, index + 2)?;
            if closing != name {
                return Err(MatchError {
                    span: open,
                    message: format!("`<{name}>` is closed by `</{closing}>`"),
                });
            }
            if !punct(tokens.get(after), '>') {
                return Err(unclosed());
            }
            return Ok((after + 1, tokens[after].span()));
        }
        index += child(&tokens[index..], state)?;
    }
}

/// Matches one child; returns the tokens it occupies.
fn child(tokens: &[TokenTree], state: &mut State) -> Result<usize> {
    if punct(tokens.first(), '<') && punct(tokens.get(1), '>') {
        let mut index = 2;
        loop {
            if index >= tokens.len() {
                return Err(MatchError {
                    span: tokens[0].span(),
                    message: "`<>` is never closed by `</>`".into(),
                });
            }
            if punct(tokens.get(index), '<')
                && punct(tokens.get(index + 1), '/')
                && punct(tokens.get(index + 2), '>')
            {
                return Ok(index + 3);
            }
            index += child(&tokens[index..], state)?;
        }
    }
    if punct(tokens.first(), '<') {
        return element(tokens, state).map(|(len, _)| len);
    }
    if let Some(group) = brace(tokens.first()) {
        state.rust.push(group.stream());
        return Ok(1);
    }
    match ident(tokens.first()).map(ToString::to_string).as_deref() {
        Some("if") => if_child(tokens, state),
        Some("match") => {
            let (scrutinee, body_at) = until_brace(tokens, 1)?;
            state.rust.push(scrutinee);
            let Some(body) = brace(tokens.get(body_at)) else {
                unreachable!("until_brace found it")
            };
            arms(&body.stream().into_iter().collect::<Vec<_>>(), state)?;
            Ok(body_at + 1)
        }
        Some("for") => {
            let mut index = 1;
            while ident(tokens.get(index)).is_none_or(|word| word != "in") {
                if index >= tokens.len() {
                    return Err(MatchError {
                        span: tokens[0].span(),
                        message: "expected `in` in `for`".into(),
                    });
                }
                index += 1;
            }
            let (iterated, body_at) = until_brace(tokens, index + 1)?;
            state.rust.push(iterated);
            let Some(body) = brace(tokens.get(body_at)) else {
                unreachable!("until_brace found it")
            };
            children(&body.stream().into_iter().collect::<Vec<_>>(), state)?;
            Ok(body_at + 1)
        }
        _ => Err(MatchError {
            span: tokens[0].span(),
            message: "expected an element, `{expression}`, `if`, `match`, `for`, or a closing tag \
                      (text is written as `text=\"…\"`)"
                .into(),
        }),
    }
}

fn if_child(tokens: &[TokenTree], state: &mut State) -> Result<usize> {
    let (condition, body_at) = until_brace(tokens, 1)?;
    state.rust.push(condition);
    let Some(body) = brace(tokens.get(body_at)) else { unreachable!("until_brace found it") };
    children(&body.stream().into_iter().collect::<Vec<_>>(), state)?;
    let mut index = body_at + 1;
    if ident(tokens.get(index)).is_some_and(|word| word == "else") {
        index += 1;
        if ident(tokens.get(index)).is_some_and(|word| word == "if") {
            index += if_child(&tokens[index..], state)?;
        } else if let Some(body) = brace(tokens.get(index)) {
            children(&body.stream().into_iter().collect::<Vec<_>>(), state)?;
            index += 1;
        }
    }
    Ok(index)
}

fn children(tokens: &[TokenTree], state: &mut State) -> Result<()> {
    let mut index = 0;
    while index < tokens.len() {
        index += child(&tokens[index..], state)?;
    }
    Ok(())
}

fn arms(tokens: &[TokenTree], state: &mut State) -> Result<()> {
    let mut index = 0;
    while index < tokens.len() {
        // The pattern (and guard) run to `=>`.
        let start = index;
        while !(punct(tokens.get(index), '=') && punct(tokens.get(index + 1), '>')) {
            if index >= tokens.len() {
                return Err(MatchError {
                    span: tokens[start].span(),
                    message: "expected `=>` in a match arm".into(),
                });
            }
            index += 1;
        }
        state.rust.push(tokens[start..index].iter().cloned().collect());
        index += 2;
        if let Some(body) = brace(tokens.get(index)) {
            children(&body.stream().into_iter().collect::<Vec<_>>(), state)?;
            index += 1;
        } else {
            index += child(&tokens[index..], state)?;
        }
        if punct(tokens.get(index), ',') {
            index += 1;
        }
    }
    Ok(())
}

/// The tokens from `from` up to the next brace group at this level, and the
/// group's index.
fn until_brace(tokens: &[TokenTree], from: usize) -> Result<(TokenStream, usize)> {
    let mut index = from;
    while brace(tokens.get(index)).is_none() {
        if index >= tokens.len() {
            return Err(MatchError {
                span: tokens[0].span(),
                message: "expected a `{ … }` body".into(),
            });
        }
        index += 1;
    }
    Ok((tokens[from..index].iter().cloned().collect(), index))
}
