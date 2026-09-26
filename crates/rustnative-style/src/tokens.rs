//! Model values as the Rust expressions that construct them — what the
//! `classes!`/`styles!` macros and `rustnative_build::compile_styles` write.
//! `krate` is the path the output names the model through, normally
//! `::rustnative_core`, which re-exports it as `style::decl`.

use proc_macro2::TokenStream;
use quote::quote;

use crate::model::{
    Color, Condition, ConditionalDeclaration, Direction, Fixed, Keyword, Length, Pointer, Scheme,
    ShadowLayer, State, StyleProperty, StyleValue,
};

fn ident(name: &str) -> proc_macro2::Ident {
    proc_macro2::Ident::new(name, proc_macro2::Span::call_site())
}

fn fixed(value: Fixed, path: &TokenStream) -> TokenStream {
    let milli = value.milli();
    quote!(#path::Fixed::from_milli(#milli))
}

fn color(value: Color, path: &TokenStream) -> TokenStream {
    let Color { red, green, blue, alpha } = value;
    quote!(#path::Color::rgba(#red, #green, #blue, #alpha))
}

fn option<T>(value: Option<T>, emit: impl Fn(T) -> TokenStream) -> TokenStream {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |value| {
            let inner = emit(value);
            quote!(::core::option::Option::Some(#inner))
        },
    )
}

fn condition(value: &Condition, path: &TokenStream) -> TokenStream {
    let state = option(value.state, |state| {
        let variant = ident(match state {
            State::Hover => "Hover",
            State::Focus => "Focus",
            State::FocusVisible => "FocusVisible",
            State::Active => "Active",
            State::Disabled => "Disabled",
        });
        quote!(#path::State::#variant)
    });
    let scheme = option(value.scheme, |scheme| {
        let variant = ident(match scheme {
            Scheme::Light => "Light",
            Scheme::Dark => "Dark",
        });
        quote!(#path::Scheme::#variant)
    });
    let min_width = option(value.min_width, |width| quote!(#width));
    let direction = option(value.direction, |direction| {
        let variant = ident(match direction {
            Direction::Ltr => "Ltr",
            Direction::Rtl => "Rtl",
        });
        quote!(#path::Direction::#variant)
    });
    let reduced_motion = option(value.reduced_motion, |reduced| quote!(#reduced));
    let pointer = option(value.pointer, |pointer| {
        let variant = ident(match pointer {
            Pointer::Fine => "Fine",
            Pointer::Coarse => "Coarse",
        });
        quote!(#path::Pointer::#variant)
    });
    quote!(#path::Condition {
        state: #state,
        scheme: #scheme,
        min_width: #min_width,
        direction: #direction,
        reduced_motion: #reduced_motion,
        pointer: #pointer,
    })
}

fn property(value: StyleProperty, path: &TokenStream) -> TokenStream {
    let variant = ident(&format!("{value:?}"));
    quote!(#path::StyleProperty::#variant)
}

fn shadow(layer: &ShadowLayer, path: &TokenStream) -> TokenStream {
    let (x, y, blur, spread) = (
        fixed(layer.x, path),
        fixed(layer.y, path),
        fixed(layer.blur, path),
        fixed(layer.spread, path),
    );
    let color = color(layer.color, path);
    let inset = layer.inset;
    quote!(#path::ShadowLayer { x: #x, y: #y, blur: #blur, spread: #spread, color: #color, inset: #inset })
}

/// A value's constructor.
#[must_use]
pub fn value(value: &StyleValue, krate: &TokenStream) -> TokenStream {
    let path = quote!(#krate::style::decl);
    let path = &path;
    match value {
        StyleValue::Color(c) => {
            let c = color(*c, path);
            quote!(#path::StyleValue::Color(#c))
        }
        StyleValue::Length(length) => {
            let (unit, amount) = match length {
                Length::Px(v) => ("Px", v),
                Length::Rem(v) => ("Rem", v),
                Length::Em(v) => ("Em", v),
            };
            let unit = ident(unit);
            let amount = fixed(*amount, path);
            quote!(#path::StyleValue::Length(#path::Length::#unit(#amount)))
        }
        StyleValue::Number(n) => {
            let n = fixed(*n, path);
            quote!(#path::StyleValue::Number(#n))
        }
        StyleValue::Keyword(keyword) => {
            let variant = ident(match keyword {
                Keyword::Auto => "Auto",
                Keyword::Fill => "Fill",
                Keyword::Start => "Start",
                Keyword::Center => "Center",
                Keyword::End => "End",
                Keyword::Stretch => "Stretch",
                Keyword::Visible => "Visible",
                Keyword::Clip => "Clip",
                Keyword::Scroll => "Scroll",
                Keyword::Hidden => "Hidden",
                Keyword::Shown => "Shown",
            });
            quote!(#path::StyleValue::Keyword(#path::Keyword::#variant))
        }
        StyleValue::Family(family) => {
            let family = family.as_ref();
            quote!(#path::StyleValue::Family(::std::borrow::Cow::Borrowed(#family)))
        }
        StyleValue::Shadow(layers) => {
            let layers = layers.iter().map(|layer| shadow(layer, path));
            quote!(#path::StyleValue::Shadow(::std::borrow::Cow::Borrowed(&[#(#layers),*])))
        }
        StyleValue::Token(name) => {
            let name = name.as_ref();
            quote!(#path::StyleValue::Token(::std::borrow::Cow::Borrowed(#name)))
        }
        StyleValue::Scaled(name, factor) => {
            let name = name.as_ref();
            let factor = fixed(*factor, path);
            quote!(#path::StyleValue::Scaled(::std::borrow::Cow::Borrowed(#name), #factor))
        }
        StyleValue::Faded(name, percent) => {
            let name = name.as_ref();
            quote!(#path::StyleValue::Faded(::std::borrow::Cow::Borrowed(#name), #percent))
        }
    }
}

/// A `DeclarationSet` expression over a `static` holding `declarations`,
/// with `prelude` (items or statements) at the start of its block.
/// `sources` names the class or declaration each came from (one per
/// declaration, or empty when unknown); `declared` says the set was
/// written as declarations rather than classes.
#[must_use]
pub fn declaration_set(
    declarations: &[ConditionalDeclaration],
    sources: &[String],
    declared: bool,
    krate: &TokenStream,
    prelude: &TokenStream,
) -> TokenStream {
    let path = quote!(#krate::style::decl);
    let count = declarations.len();
    let items = declarations.iter().map(|declaration| {
        let condition = condition(&declaration.condition, &path);
        let property = property(declaration.declaration.property, &path);
        let value = value(&declaration.declaration.value, krate);
        quote!(#path::ConditionalDeclaration {
            condition: #condition,
            declaration: #path::Declaration { property: #property, value: #value },
        })
    });
    let sources = if sources.len() == count { sources } else { &[] };
    let source_count = sources.len();
    let kind = if declared { quote!(Declarations) } else { quote!(Classes) };
    quote!({
        #prelude
        static DECLARATIONS: [#path::ConditionalDeclaration; #count] = [#(#items),*];
        static SOURCES: [&str; #source_count] = [#(#sources),*];
        #path::DeclarationSet::from_sources(&DECLARATIONS, &SOURCES, #path::SetKind::#kind)
    })
}

/// A `TokenTable` expression: the defaults, with `cleared` namespaces
/// removed and `set` tokens written over them.
#[must_use]
pub fn token_table(
    cleared: &[String],
    set: &[(String, StyleValue)],
    krate: &TokenStream,
) -> TokenStream {
    let path = quote!(#krate::style::decl);
    let set = set.iter().map(|(name, token)| {
        let token = value(token, krate);
        quote!(.with(#name, #token))
    });
    quote!(#path::TokenTable::defaults() #(.without_namespace(#cleared))* #(#set)*)
}
