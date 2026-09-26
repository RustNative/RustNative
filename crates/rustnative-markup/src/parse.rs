//! Parsing markup from Rust tokens.
//!
//! Markup is parsed from a token stream, not from text: a `rsx!` body is
//! already tokens, and a `.rsx` file is tokenized by the host language's
//! own lexer (`proc_macro2`). That is why the grammar has no bare text —
//! loose prose is not a token stream — and why it is identical in both
//! carriers by construction.

use proc_macro2::{Span, TokenStream};
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream, Parser};
use syn::{Expr, Ident, Lit, Pat, Path, Token, braced};

use crate::ast::{Attr, AttrValue, Child, Element, ElseBranch, IfChild, Markup, MatchArm};

impl Parse for Markup {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let context = if input.peek(Token![in]) {
            input.parse::<Token![in]>()?;
            let context = input.parse::<Expr>()?;
            input.parse::<Token![,]>().map_err(|error| {
                syn::Error::new(
                    error.span(),
                    "expected `,` after the component context: `in context, <Element …/>`",
                )
            })?;
            Some(context)
        } else {
            None
        };
        if input.peek(Token![<]) && input.peek2(Token![>]) {
            return Err(input.error(
                "a markup expression evaluates to one `Node`, so its root must be an element, not a fragment",
            ));
        }
        if !input.peek(Token![<]) {
            return Err(input.error("expected an element: `<Column key=\"…\">…</Column>`"));
        }
        let root = parse_element(input)?;
        if !input.is_empty() {
            return Err(input.error(
                "a markup expression has exactly one root element; wrap several in a container",
            ));
        }
        Ok(Self { context, root })
    }
}

/// Parses one markup expression (what `rsx!` receives).
///
/// # Errors
///
/// The tokens are not a markup expression; the error is spanned to the
/// offending token.
pub fn parse_markup(tokens: TokenStream) -> syn::Result<Markup> {
    syn::parse2(tokens)
}

/// Parses one element from the start of `tokens`, returning it and the
/// tokens after it — how the `.rsx` compiler finds where an element ends.
///
/// # Errors
///
/// The tokens do not start with a well-formed element.
pub fn parse_leading_element(tokens: TokenStream) -> syn::Result<(Element, TokenStream)> {
    let parser = |input: ParseStream<'_>| {
        let element = parse_element(input)?;
        let rest: TokenStream = input.parse()?;
        Ok((element, rest))
    };
    parser.parse2(tokens)
}

pub(crate) fn parse_element(input: ParseStream<'_>) -> syn::Result<Element> {
    let open = input.parse::<Token![<]>()?;
    let name: Path = input
        .call(Path::parse_mod_style)
        .map_err(|error| syn::Error::new(error.span(), "expected an element name after `<`"))?;
    let mut attrs = Vec::new();
    let mut spreads = Vec::new();
    loop {
        if input.is_empty() {
            return Err(syn::Error::new(
                open.span,
                format!("`<{}>` is never closed", path_string(&name)),
            ));
        }
        if input.peek(Token![/]) || input.peek(Token![>]) {
            break;
        }
        if input.peek(Token![..]) {
            input.parse::<Token![..]>()?;
            let spread = if input.peek(syn::token::Brace) {
                let content;
                braced!(content in input);
                parse_whole_expr(&content)?
            } else {
                Expr::Path(input.parse()?)
            };
            spreads.push(spread);
            continue;
        }
        if input.peek(Lit) {
            return Err(input.error("expected an attribute name; text is written as `text=\"…\"`"));
        }
        let attr_name = Ident::parse_any(input).map_err(|error| {
            syn::Error::new(error.span(), "expected an attribute name, `/>`, or `>`")
        })?;
        let value = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            if input.peek(syn::token::Brace) {
                let content;
                braced!(content in input);
                AttrValue::Expr(parse_whole_expr(&content)?)
            } else if input.peek(Lit) {
                AttrValue::Lit(input.parse()?)
            } else if input.peek(Token![-]) {
                // A negative number: `margin=-4`.
                let minus = input.parse::<Token![-]>()?;
                let lit: Lit = input.parse()?;
                AttrValue::Expr(syn::parse_quote_spanned!(minus.span=> -#lit))
            } else {
                return Err(input.error(format!(
                    "expected a value for `{attr_name}`: a literal (`{attr_name}=\"…\"`) or a braced expression (`{attr_name}={{…}}`)"
                )));
            }
        } else {
            AttrValue::Flag
        };
        if attrs.iter().any(|existing: &Attr| existing.name == attr_name)
            && attr_name != "transition"
        {
            return Err(syn::Error::new(attr_name.span(), format!("`{attr_name}` is set twice")));
        }
        attrs.push(Attr { name: attr_name, value });
    }
    if input.peek(Token![/]) {
        input.parse::<Token![/]>()?;
        let close = input.parse::<Token![>]>()?;
        return Ok(Element {
            name,
            attrs,
            spreads,
            children: Vec::new(),
            open_span: open.span,
            close_span: close.span,
            self_closing: true,
        });
    }
    input.parse::<Token![>]>()?;
    let children = parse_children_until_close(input, &name, open.span)?;
    input.parse::<Token![<]>()?;
    input.parse::<Token![/]>()?;
    let closing: Path = input.call(Path::parse_mod_style)?;
    if path_string(&closing) != path_string(&name) {
        return Err(syn::Error::new(
            open.span,
            format!(
                "`<{}>` is closed by `</{}>`; close it with `</{}>`",
                path_string(&name),
                path_string(&closing),
                path_string(&name)
            ),
        ));
    }
    let close = input.parse::<Token![>]>()?;
    Ok(Element {
        name,
        attrs,
        spreads,
        children,
        open_span: open.span,
        close_span: close.span,
        self_closing: false,
    })
}

fn parse_children_until_close(
    input: ParseStream<'_>,
    name: &Path,
    open: Span,
) -> syn::Result<Vec<Child>> {
    let mut children = Vec::new();
    loop {
        if input.is_empty() {
            return Err(syn::Error::new(
                open,
                format!("`<{}>` is never closed", path_string(name)),
            ));
        }
        if input.peek(Token![<]) && input.peek2(Token![/]) {
            return Ok(children);
        }
        children.push(parse_child(input)?);
    }
}

fn parse_children_to_end(input: ParseStream<'_>) -> syn::Result<Vec<Child>> {
    let mut children = Vec::new();
    while !input.is_empty() {
        children.push(parse_child(input)?);
    }
    Ok(children)
}

fn parse_child(input: ParseStream<'_>) -> syn::Result<Child> {
    if input.peek(Token![<]) && input.peek2(Token![>]) {
        let open = input.parse::<Token![<]>()?;
        input.parse::<Token![>]>()?;
        let mut children = Vec::new();
        loop {
            if input.is_empty() {
                return Err(syn::Error::new(open.span, "`<>` is never closed by `</>`"));
            }
            if input.peek(Token![<]) && input.peek2(Token![/]) && input.peek3(Token![>]) {
                input.parse::<Token![<]>()?;
                input.parse::<Token![/]>()?;
                input.parse::<Token![>]>()?;
                return Ok(Child::Fragment(children));
            }
            children.push(parse_child(input)?);
        }
    }
    if input.peek(Token![<]) {
        return Ok(Child::Element(parse_element(input)?));
    }
    if input.peek(syn::token::Brace) {
        let content;
        braced!(content in input);
        return Ok(Child::Expr(parse_whole_expr(&content)?));
    }
    if input.peek(Token![if]) {
        return Ok(Child::If(parse_if(input)?));
    }
    if input.peek(Token![match]) {
        input.parse::<Token![match]>()?;
        let expr = Expr::parse_without_eager_brace(input)?;
        let content;
        braced!(content in input);
        let mut arms = Vec::new();
        while !content.is_empty() {
            let pat = Pat::parse_multi_with_leading_vert(&content)?;
            let guard = if content.peek(Token![if]) {
                content.parse::<Token![if]>()?;
                Some(content.parse::<Expr>()?)
            } else {
                None
            };
            content.parse::<Token![=>]>()?;
            let body = if content.peek(syn::token::Brace) {
                let inner;
                braced!(inner in content);
                parse_children_to_end(&inner)?
            } else {
                vec![parse_child(&content)?]
            };
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
            arms.push(MatchArm { pat, guard, body });
        }
        return Ok(Child::Match { expr, arms });
    }
    if input.peek(Token![for]) {
        input.parse::<Token![for]>()?;
        let pat = Pat::parse_multi_with_leading_vert(input)?;
        input.parse::<Token![in]>()?;
        let expr = Expr::parse_without_eager_brace(input)?;
        let content;
        braced!(content in input);
        return Ok(Child::For { pat, expr, body: parse_children_to_end(&content)? });
    }
    if input.peek(Lit) {
        return Err(input.error(
            "text is not allowed between tags: write it as an attribute (`text=\"…\"`) or a braced expression (`{…}`)",
        ));
    }
    Err(input.error("expected an element, `{expression}`, `if`, `match`, `for`, or a closing tag"))
}

fn parse_if(input: ParseStream<'_>) -> syn::Result<IfChild> {
    input.parse::<Token![if]>()?;
    let cond = Expr::parse_without_eager_brace(input)?;
    let content;
    braced!(content in input);
    let then = parse_children_to_end(&content)?;
    let otherwise = if input.peek(Token![else]) {
        input.parse::<Token![else]>()?;
        if input.peek(Token![if]) {
            Some(Box::new(ElseBranch::If(parse_if(input)?)))
        } else {
            let content;
            braced!(content in input);
            Some(Box::new(ElseBranch::Block(parse_children_to_end(&content)?)))
        }
    } else {
        None
    };
    Ok(IfChild { cond, then, otherwise })
}

fn parse_whole_expr(input: ParseStream<'_>) -> syn::Result<Expr> {
    if input.is_empty() {
        return Err(input.error("expected an expression inside the braces"));
    }
    let expr = input.parse::<Expr>()?;
    if !input.is_empty() {
        return Err(input.error("expected one expression inside the braces"));
    }
    Ok(expr)
}

pub(crate) fn path_string(path: &Path) -> String {
    path.segments.iter().map(|segment| segment.ident.to_string()).collect::<Vec<_>>().join("::")
}
