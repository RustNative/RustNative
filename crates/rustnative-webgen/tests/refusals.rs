//! Code outside the client subset is a compile error at the construct,
//! naming the three ways forward (`PLAN.md` Web milestone A).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    reason = "tests"
)]

use quote::quote;

fn refusals(body: proc_macro2::TokenStream) -> Vec<String> {
    let module = quote! {
        mod sample {
            use rustnative_core::{Event, Node};
            use rustnative_web::Effects;
            #[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
            pub struct State { pub count: i32, pub text: String, pub big: i64, pub items: Vec<i32> }
            impl State {
                pub fn update(&mut self, event: Event, fx: &mut Effects<()>) { #body }
                pub fn view(&self) -> Node { Node::label("x", "x") }
            }
        }
    };
    let mut module: syn::ItemMod = syn::parse2(module).unwrap();
    match rustnative_webgen::translate(&mut module) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.iter().map(ToString::to_string).collect(),
    }
}

fn refused(body: proc_macro2::TokenStream, what: &str) {
    let errors = refusals(body.clone());
    assert!(!errors.is_empty(), "`{body}` was accepted");
    let first = &errors[0];
    assert!(first.contains(what), "`{body}`: expected a refusal about {what}, got: {first}");
    assert!(
        first.contains("#[server]")
            && first.contains("WebAssembly subtree")
            && first.contains("fx.js"),
        "the refusal names the three ways forward: {first}"
    );
}

#[test]
fn the_subset_accepts_ordinary_client_logic() {
    let errors = refusals(quote! {
        let _ = fx;
        if let Event::Click { target } = event {
            if target == rustnative_core::NodeId::from_key("add") {
                self.count += 1;
                self.items.push(self.count * 2);
                self.text = format!("{} items, {} next", self.items.len(), self.items.len() + 1);
            }
        }
    });
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn a_literal_inside_a_macros_arguments_gets_its_type() {
    let module = quote! {
        mod sample {
            use rustnative_core::{Event, Node};
            use rustnative_web::Effects;
            #[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
            pub struct State { pub text: String, pub items: Vec<i32> }
            impl State {
                pub fn update(&mut self, _event: Event, _fx: &mut Effects<()>) {
                    self.text = format!("{}", self.items.len() + 1);
                }
                pub fn view(&self) -> Node { Node::label("x", "x") }
            }
        }
    };
    let mut module: syn::ItemMod = syn::parse2(module).unwrap();
    rustnative_webgen::translate(&mut module).unwrap();
    let rust = quote!(#module).to_string();
    assert!(!rust.contains("__rn_lit"), "{rust}");
    assert!(rust.contains("1usize") || rust.contains("1_usize"), "{rust}");
}

#[test]
fn each_construct_outside_the_subset_is_refused() {
    refused(quote! { let _ = std::fs::read_to_string("x"); }, "`fs::read_to_string`");
    refused(quote! { let x = Some(1)?; }, "`?` operator");
    refused(quote! { let _ = self.count << 2; }, "`<<`");
    refused(quote! { let _ = &self.text[0..2]; }, "slicing");
    refused(quote! { let _ = self.text.find("a"); }, "`.find()` on String");
    refused(quote! { let _ = 9_007_199_254_740_993_i64; }, "this literal");
    refused(quote! { let _ = i64::MAX; }, "`i64::MAX`");
    refused(quote! { let _ = async { 1 }; }, "asynchronous code");
    refused(quote! { unsafe { let _ = 1; } }, "`unsafe`");
    refused(quote! { fn inner() {} }, "an item inside a function");
    refused(quote! { let _ = std::thread::spawn(|| ()); }, "`thread::spawn`");
    refused(quote! { let _ = format!("{:?}", 1.5_f64); }, "`{:?}` of f64");
}

#[test]
fn serde_renames_are_refused_because_the_generated_code_reads_rust_names() {
    let module = quote! {
        mod sample {
            use rustnative_core::{Event, Node};
            use rustnative_web::Effects;
            #[derive(Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
            pub struct State { #[serde(rename = "c")] pub count: i32 }
            impl State {
                pub fn update(&mut self, _event: Event, _fx: &mut Effects<()>) {}
                pub fn view(&self) -> Node { Node::label("x", "x") }
            }
        }
    };
    let mut module: syn::ItemMod = syn::parse2(module).unwrap();
    let errors = rustnative_webgen::translate(&mut module).expect_err("refused");
    assert!(errors[0].to_string().contains("serde(rename)"), "{}", errors[0]);
}

#[test]
fn a_module_without_view_and_update_is_explained() {
    let mut module: syn::ItemMod = syn::parse2(quote! { mod empty { pub struct State; } }).unwrap();
    let errors = rustnative_webgen::translate(&mut module).expect_err("refused");
    assert!(errors[0].to_string().contains("fn view(&self) -> Node"), "{}", errors[0]);
}
