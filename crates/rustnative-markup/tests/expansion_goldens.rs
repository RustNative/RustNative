//! Expansion goldens: what each markup construct lowers to, pretty-printed
//! and compared with a reviewed file, so a change to the lowering is a
//! visible diff — and so `PLAN.md` 2.9's claim that markup "expands to
//! builder calls and nothing else" can be checked by reading them. Bless
//! with `RUSTNATIVE_BLESS=1`.

use std::path::Path;

use quote::quote;

#[allow(clippy::expect_used, reason = "a test helper: a failure is the test failing")]
fn check(name: &str, tokens: proc_macro2::TokenStream) {
    let lowered = rustnative_markup::expand(tokens).expect("the case lowers");
    let file: syn::File =
        syn::parse2(quote!(fn expansion() -> ::rustnative_core::Node { #lowered }))
            .expect("the lowering is a valid expression");
    let actual = prettyplease::unparse(&file);
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expansions").join(format!("{name}.rs"));
    if std::env::var("RUSTNATIVE_BLESS").is_ok_and(|value| value == "1") {
        std::fs::write(&path, &actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(
        expected, actual,
        "expansion of `{name}` changed; bless with RUSTNATIVE_BLESS=1 if intended"
    );
}

#[test]
fn leaf_with_layout_and_modifiers() {
    check(
        "leaf",
        quote! { <Label key="title" text={title} width={SizeMode::Fixed(80)} opacity=0.5 disabled /> },
    );
}

#[test]
fn container_with_children_and_control_flow() {
    check(
        "container",
        quote! {
            <Column key="list" gap=4>
                if show { <Label key="a" text="A" /> } else { <Label key="b" text="B" /> }
                for item in items { <Label key={item} text={item} /> }
                {extra}
            </Column>
        },
    );
}

#[test]
fn component_element_and_spread() {
    check("component", quote! { in context, <Card key="card" title="Hi" ..{emphasized} /> });
}
