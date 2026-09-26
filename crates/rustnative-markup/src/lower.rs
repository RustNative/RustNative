//! Lowering markup to builder calls — and nothing else.
//!
//! Every element becomes a `Node` constructor call, every attribute the
//! builder method of the same name, every child construct the Rust control
//! flow it reads as. The output introduces no node kind, no runtime type,
//! and no helper beyond `IntoChildren` (which only extends a `Vec<Node>`),
//! which is what makes `PLAN.md` 2.9's "expands to builder calls and nothing
//! else" checkable in the expansion goldens.
//!
//! Generated tokens carry the spans of the markup they came from, so a type
//! error in an attribute's value is reported at that value, and a wrong
//! element or attribute at its name.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{Expr, Lit};

use crate::ast::{
    Attr, AttrValue, Child, Element, ElseBranch, IfChild, Markup, contains_component,
};
use crate::table::{AttrKind, Constructor, element_spec, nearest};

/// Lowers `markup` to the builder expression it means.
///
/// # Errors
///
/// An unknown element attribute, a missing required attribute, a flag
/// written where a value is needed, children on an element that takes
/// none, or a component element with no context — each spanned to what
/// the developer wrote.
pub fn lower(markup: &Markup) -> syn::Result<TokenStream> {
    let context = markup.context.as_ref();
    if context.is_none() && contains_component(&markup.root) {
        let offending = first_component(&markup.root)
            .map_or(markup.root.open_span, |element| element.open_span);
        return Err(syn::Error::new(
            offending,
            "a component element needs the component context: write `rsx! { in context, … }` \
             (a `.rsx` file finds it from the enclosing function's `ComponentContext` parameter)",
        ));
    }
    lower_element(&markup.root, context)
}

fn first_component(element: &Element) -> Option<&Element> {
    if !crate::table::is_builtin(&element.name_string()) {
        return Some(element);
    }
    element.children.iter().find_map(|child| match child {
        Child::Element(inner) => first_component(inner),
        _ => None,
    })
}

fn lower_element(element: &Element, context: Option<&Expr>) -> syn::Result<TokenStream> {
    let name = element.name_string();
    let Some(spec) = element_spec(&name) else {
        return lower_component(element, context);
    };
    let span =
        element.name.segments.last().map_or(element.open_span, |segment| segment.ident.span());

    // Unknown attributes, with the nearest known one.
    for attr in &element.attrs {
        let attr_name = attr.name.to_string();
        let Some(attr_spec) = spec.attrs.iter().find(|candidate| candidate.name == attr_name)
        else {
            let suggestion = nearest(&attr_name, spec.attrs.iter().map(|candidate| candidate.name))
                .and_then(|near| spec.attrs.iter().find(|candidate| candidate.name == near))
                .map_or_else(String::new, |near| {
                    format!("; did you mean `{}` ({})?", near.name, near.method)
                });
            return Err(syn::Error::new(
                attr.name.span(),
                format!("`<{name}>` has no attribute `{attr_name}`{suggestion}"),
            ));
        };
        if matches!(attr.value, AttrValue::Flag) && !attr_spec.flag {
            return Err(syn::Error::new(
                attr.name.span(),
                format!("`{attr_name}` needs a value: `{attr_name}=\"…\"` or `{attr_name}={{…}}`"),
            ));
        }
    }
    for required in spec.attrs.iter().filter(|attr| attr.required) {
        if element.attr(required.name).is_none() {
            return Err(syn::Error::new(span, format!("`<{name}>` needs `{}`", required.name)));
        }
    }
    if !spec.children && !element.children.is_empty() {
        return Err(syn::Error::new(
            span,
            format!("`<{name}>` takes no children; its content is an attribute"),
        ));
    }

    let arg = |attr_name: &str| -> TokenStream {
        element.attr(attr_name).map_or_else(TokenStream::new, |attr| value_tokens(&attr.value))
    };
    let key = arg("key");

    let mut layout = quote_spanned!(span=> <::rustnative_core::LayoutStyle as ::core::default::Default>::default());
    let mut container = TokenStream::new();
    let style_type = match spec.constructor {
        Constructor::Column => Some(quote_spanned!(span=> ::rustnative_core::ColumnStyle)),
        Constructor::Row => Some(quote_spanned!(span=> ::rustnative_core::RowStyle)),
        _ => None,
    };
    if let Some(style_type) = &style_type {
        container = quote_spanned!(span=> <#style_type as ::core::default::Default>::default());
    }
    let mut modifiers = TokenStream::new();
    for attr in &element.attrs {
        let attr_name = attr.name.to_string();
        let Some(attr_spec) = spec.attrs.iter().find(|candidate| candidate.name == attr_name)
        else {
            continue;
        };
        let method = format_ident!("{}", attr_name, span = attr.name.span());
        let value = value_tokens(&attr.value);
        match attr_spec.kind {
            AttrKind::Key | AttrKind::Argument => {}
            AttrKind::Layout => {
                layout = quote_spanned!(attr.value.span()=> #layout.#method(#value));
            }
            AttrKind::Container => {
                container = quote_spanned!(attr.value.span()=> #container.#method(#value));
            }
            AttrKind::Modifier => modifiers.extend(lower_modifier(attr, &attr_name, &value)),
        }
    }

    let children =
        if spec.children { Some(lower_children(&element.children, context)?) } else { None };
    let children_tokens = children.unwrap_or_default();
    let construct = match spec.constructor {
        Constructor::Label => {
            let text = arg("text");
            quote_spanned!(span=> ::rustnative_core::Node::label_with_layout(#key, #text, __layout))
        }
        Constructor::Button => {
            let text = arg("text");
            quote_spanned!(span=> ::rustnative_core::Node::button_with_layout(#key, #text, __layout))
        }
        Constructor::TextInput => {
            let value = arg("value");
            quote_spanned!(span=> ::rustnative_core::Node::text_input_with_layout(#key, #value, __layout))
        }
        Constructor::Canvas => {
            let draw_list = arg("draw_list");
            quote_spanned!(span=> ::rustnative_core::Node::canvas(#key, #draw_list, __layout))
        }
        Constructor::Surface => {
            quote_spanned!(span=> ::rustnative_core::Node::native_surface(#key, __layout))
        }
        Constructor::Foreign => {
            let kind = arg("kind");
            quote_spanned!(span=> ::rustnative_core::Node::foreign(#key, #kind, __layout))
        }
        Constructor::TabBar => {
            let labels = arg("labels");
            let selected = arg("selected");
            quote_spanned!(span=> ::rustnative_core::Node::tab_bar(#key, #labels, #selected, __layout))
        }
        Constructor::Checkbox => {
            let label = arg("label");
            let checked = arg("checked");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Checkbox { label: ::core::convert::Into::into(#label), checked: #checked }, __layout))
        }
        Constructor::Radio => {
            let label = arg("label");
            let selected = arg("selected");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Radio { label: ::core::convert::Into::into(#label), selected: #selected }, __layout))
        }
        Constructor::Toggle => {
            let label = arg("label");
            let on = arg("on");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Toggle { label: ::core::convert::Into::into(#label), on: #on }, __layout))
        }
        Constructor::Slider => {
            let value = arg("value");
            let min = arg("min");
            let max = arg("max");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Slider { value: #value, min: #min, max: #max }, __layout))
        }
        Constructor::Progress => {
            let percent = arg("percent");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Progress { percent: #percent }, __layout))
        }
        Constructor::Select => {
            let options = arg("options");
            let selected = arg("selected");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Select { options: ::core::iter::IntoIterator::into_iter(#options).map(::core::convert::Into::into).collect(), selected: #selected }, __layout))
        }
        Constructor::ListBox => {
            let items = arg("items");
            let selected = arg("selected");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::ListBox { items: ::core::iter::IntoIterator::into_iter(#items).map(::core::convert::Into::into).collect(), selected: #selected }, __layout))
        }
        Constructor::DatePicker => {
            let date = arg("date");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::DatePicker { date: #date }, __layout))
        }
        Constructor::Spinner => {
            let value = arg("value");
            let min = arg("min");
            let max = arg("max");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Spinner { value: #value, min: #min, max: #max }, __layout))
        }
        Constructor::Separator => {
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Separator, __layout))
        }
        Constructor::Link => {
            let text = arg("text");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Link { text: ::core::convert::Into::into(#text) }, __layout))
        }
        Constructor::MultilineText => {
            let value = arg("value");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::MultilineText { value: ::core::convert::Into::into(#value) }, __layout))
        }
        Constructor::Image => {
            let image = arg("image");
            quote_spanned!(span=> ::rustnative_core::Node::control_with_layout(#key, ::rustnative_core::Control::Image { image: #image }, __layout))
        }
        Constructor::Column => quote_spanned!(span=>
            ::rustnative_core::Node::column_with_layout(#key, #children_tokens, __layout, __container)
        ),
        Constructor::Row => quote_spanned!(span=>
            ::rustnative_core::Node::row_with_layout(#key, #children_tokens, __layout, __container)
        ),
        Constructor::VirtualList => {
            let list = arg("list");
            quote_spanned!(span=>
                ::rustnative_core::Node::virtual_list_with_layout(#key, #list, __layout, #children_tokens)
            )
        }
        Constructor::Grid => {
            let tracks = arg("tracks");
            quote_spanned!(span=> ::rustnative_core::Node::grid(#key, #tracks, __layout, #children_tokens))
        }
    };
    let container_binding = if style_type.is_some() {
        quote!(let __container = #container;)
    } else {
        TokenStream::new()
    };
    let spreads = element.spreads.iter().map(|spread| {
        quote_spanned!(syn::spanned::Spanned::span(spread)=> let __node = (#spread)(__node);)
    });
    Ok(quote! {
        {
            let __layout = #layout;
            #container_binding
            let __node = #construct;
            #modifiers
            #(#spreads)*
            __node
        }
    })
}

fn lower_modifier(attr: &Attr, name: &str, value: &TokenStream) -> TokenStream {
    let span = attr.value.span();
    match name {
        "disabled" | "hidden" => {
            let method = format_ident!("{}", name, span = attr.name.span());
            let value =
                if matches!(attr.value, AttrValue::Flag) { quote!(true) } else { value.clone() };
            quote_spanned!(span=> let __node = __node.#method(#value);)
        }
        // A class string is compiled where it is written; a computed one
        // could only be checked at run time, so it is refused (2.14).
        "class" => {
            if let AttrValue::Lit(syn::Lit::Str(classes)) = &attr.value {
                quote_spanned!(span=> let __node = __node.with_class(::rustnative_core::classes!(#classes));)
            } else {
                quote_spanned!(span=> ::core::compile_error!(
                    "`class` takes a string literal: a class name is resolved where it is written, so a \
                     computed class would style nothing silently — choose between literal class strings with \
                     `if`, or set typed properties with `style={…}`"
                );)
            }
        }
        "style" => {
            if let AttrValue::Lit(syn::Lit::Str(declarations)) = &attr.value {
                quote_spanned!(span=>
                    let __node = __node.with_declarations(::rustnative_core::styles!(#declarations));
                )
            } else {
                quote_spanned!(span=> let __node = __node.with_style(#value);)
            }
        }
        "transition" => quote_spanned!(span=>
            let __node = {
                let (__property, __transition) = #value;
                __node.with_transition(__property, __transition)
            };
        ),
        _ => {
            let method = format_ident!("with_{}", name, span = attr.name.span());
            quote_spanned!(span=> let __node = __node.#method(#value);)
        }
    }
}

fn value_tokens(value: &AttrValue) -> TokenStream {
    match value {
        AttrValue::Flag => quote!(true),
        AttrValue::Lit(lit) => quote!(#lit),
        AttrValue::Expr(expr) => quote!(#expr),
    }
}

fn lower_children(children: &[Child], context: Option<&Expr>) -> syn::Result<TokenStream> {
    let body = lower_child_list(children, context)?;
    Ok(quote! {
        {
            let mut __children: ::std::vec::Vec<::rustnative_core::Node> = ::std::vec::Vec::new();
            #body
            __children
        }
    })
}

fn lower_child_list(children: &[Child], context: Option<&Expr>) -> syn::Result<TokenStream> {
    let mut out = TokenStream::new();
    for child in children {
        out.extend(lower_child(child, context)?);
    }
    Ok(out)
}

fn lower_child(child: &Child, context: Option<&Expr>) -> syn::Result<TokenStream> {
    Ok(match child {
        Child::Element(element) => {
            let node = lower_element(element, context)?;
            quote!(__children.push(#node);)
        }
        Child::Expr(expr) => {
            let span = syn::spanned::Spanned::span(expr);
            quote_spanned!(span=> ::rustnative_core::IntoChildren::extend_into(#expr, &mut __children);)
        }
        Child::Fragment(children) => lower_child_list(children, context)?,
        Child::If(branch) => lower_if(branch, context)?,
        Child::Match { expr, arms } => {
            let mut lowered = Vec::new();
            for arm in arms {
                let pat = &arm.pat;
                let guard = arm.guard.as_ref().map(|guard| quote!(if #guard));
                let body = lower_child_list(&arm.body, context)?;
                lowered.push(quote!(#pat #guard => { #body }));
            }
            quote!(match #expr { #(#lowered)* })
        }
        Child::For { pat, expr, body } => {
            let body = lower_child_list(body, context)?;
            quote!(for #pat in #expr { #body })
        }
    })
}

fn lower_if(branch: &IfChild, context: Option<&Expr>) -> syn::Result<TokenStream> {
    let cond = &branch.cond;
    let then = lower_child_list(&branch.then, context)?;
    let otherwise = match branch.otherwise.as_deref() {
        None => TokenStream::new(),
        Some(ElseBranch::If(next)) => {
            let next = lower_if(next, context)?;
            quote!(else #next)
        }
        Some(ElseBranch::Block(children)) => {
            let children = lower_child_list(children, context)?;
            quote!(else { #children })
        }
    };
    Ok(quote!(if #cond { #then } #otherwise))
}

fn lower_component(element: &Element, context: Option<&Expr>) -> syn::Result<TokenStream> {
    let name = &element.name;
    let span = element.open_span;
    let Some(context) = context else {
        return Err(syn::Error::new(span, "a component element needs the component context"));
    };
    if !element.children.is_empty() {
        return Err(syn::Error::new(
            span,
            format!(
                "component element `<{}>` takes no children; pass them as a prop",
                element.name_string()
            ),
        ));
    }
    let Some(key) = element.attr("key") else {
        return Err(syn::Error::new(
            span,
            format!(
                "`<{}>` needs `key`: a component's key is its identity among its siblings",
                element.name_string()
            ),
        ));
    };
    let key = value_tokens(&key.value);
    let props: Vec<&Attr> = element.attrs.iter().filter(|attr| attr.name != "key").collect();
    let props_value = if props.is_empty() {
        quote_spanned!(span=> ::core::default::Default::default())
    } else {
        let fields = props.iter().map(|attr| {
            let field = &attr.name;
            let value = match &attr.value {
                AttrValue::Flag => quote_spanned!(attr.name.span()=> true),
                AttrValue::Lit(Lit::Str(lit)) => {
                    quote_spanned!(lit.span()=> ::core::convert::Into::into(#lit))
                }
                AttrValue::Lit(lit) => quote!(#lit),
                AttrValue::Expr(expr) => quote!(#expr),
            };
            quote!(#field: #value)
        });
        // Props not written take their default, so an element names only
        // what it changes — which requires the props type to be `Default`,
        // as an element with no props already does.
        quote_spanned!(span=> {
            type __Props = <#name as ::rustnative_core::Component>::Props;
            #[allow(clippy::needless_update, reason = "every prop may be written")]
            let __props = __Props { #(#fields,)* ..::core::default::Default::default() };
            __props
        })
    };
    let spreads = element.spreads.iter().map(|spread| {
        quote_spanned!(syn::spanned::Spanned::span(spread)=> let __node = (#spread)(__node);)
    });
    let call_span: Span = span;
    Ok(quote_spanned! {call_span=>
        {
            let __node = (#context).child_with_props::<#name, _>(
                #key,
                #props_value,
                <#name as ::rustnative_core::Component>::new,
            );
            #(#spreads)*
            __node
        }
    })
}
