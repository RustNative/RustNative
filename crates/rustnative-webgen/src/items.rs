//! A client module's items — its data types, functions, and the state
//! type's methods — and the translation of each function.

use std::collections::{HashMap, HashSet};

use proc_macro2::Span;
use syn::spanned::Spanned;

use crate::emit::{Writer, ident, string};
use crate::ty::{IntK, Ty, read};
use crate::{Translation, refuse};

/// A struct's fields.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub fields: Vec<(String, Ty)>,
    /// A tuple struct (fields named `0`, `1`, …).
    pub tuple: bool,
    pub copy: bool,
    pub default: bool,
}

/// A variant's shape.
#[derive(Debug, Clone)]
pub enum Shape {
    Unit,
    Tuple(Vec<Ty>),
    Struct(Vec<(String, Ty)>),
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub variants: Vec<(String, Shape)>,
    pub copy: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// Takes `self`.
    pub method: bool,
}

/// Everything the translator knows about a client module.
#[derive(Debug, Default)]
pub struct Module {
    pub name: String,
    pub structs: HashMap<String, StructDef>,
    pub enums: HashMap<String, EnumDef>,
    pub fns: HashMap<String, FnSig>,
    pub methods: HashMap<(String, String), FnSig>,
    pub consts: HashMap<String, Ty>,
    pub message: Ty,
}

impl Module {
    pub fn is_local(&self, name: &str) -> bool {
        self.structs.contains_key(name) || self.enums.contains_key(name)
    }

    pub fn read(&self, ty: &syn::Type) -> Ty {
        read(ty, &|name| self.is_local(name))
    }

    pub fn is_copy(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Adt(name) => {
                self.structs.get(name).is_some_and(|def| def.copy)
                    || self.enums.get(name).is_some_and(|def| def.copy)
            }
            Ty::Tuple(items) => items.iter().all(|item| item.is_primitive() || self.is_copy(item)),
            _ => false,
        }
    }

    /// A JavaScript expression for `ty`'s `Default::default()`.
    pub fn default_js(&self, ty: &Ty, span: Span) -> Result<String, syn::Error> {
        Ok(match ty {
            Ty::Int(_) | Ty::Float(_) => "0".into(),
            Ty::Bool => "false".into(),
            Ty::Str => "\"\"".into(),
            Ty::Char => "\"\\u0000\"".into(),
            Ty::Unit | Ty::Opt(_) => "null".into(),
            Ty::Vec(_) => "[]".into(),
            Ty::Tuple(items) => {
                let items: Result<Vec<String>, _> = items.iter().map(|item| self.default_js(item, span)).collect();
                format!("[{}]", items?.join(", "))
            }
            Ty::Adt(name) if self.structs.get(name).is_some_and(|def| def.default) => format!("{name}$default()"),
            Ty::Adt(name) => match self.enums.get(name).and_then(|def| def.default.clone()) {
                Some(variant) => string(&variant),
                None => return Err(refuse(span, &format!("`{name}::default()`"), "the type has no `Default`")),
            },
            Ty::Layout => "rn.layout()".into(),
            Ty::Container => "rn.containerStyle()".into(),
            Ty::Visual => "{ fg: null, bg: null, border: null, radius: null, font: null, padding: null, shadow: null }".into(),
            Ty::Typography => "{ family: \"system-ui\", size: 14, weight: 400 }".into(),
            Ty::Insets => "{ top: 0, end: 0, bottom: 0, start: 0 }".into(),
            Ty::Constraints => "{ min_width: 0, max_width: null, min_height: 0, max_height: null }".into(),
            Ty::SizeMode => "\"Auto\"".into(),
            Ty::Alignment => "\"Stretch\"".into(),
            Ty::Date => "{ year: 0, month: 0, day: 0 }".into(),
            other => return Err(refuse(span, &format!("`{other}::default()`"), "the subset has no default for it")),
        })
    }
}

/// How a name is bound in a function.
#[derive(Debug, Clone)]
pub struct Binding {
    /// Its JavaScript: a variable, or for an alias of a place (`for x in
    /// v.iter_mut()` over primitives), that place.
    pub js: String,
    pub ty: Ty,
}

/// A translated expression: statements that must run first, then a
/// JavaScript expression, and its type.
#[derive(Debug, Clone)]
pub struct V {
    pub pre: Vec<String>,
    pub js: String,
    pub ty: Ty,
}

impl V {
    pub fn new(js: impl Into<String>, ty: Ty) -> Self {
        Self { pre: Vec::new(), js: js.into(), ty }
    }
}

/// One function's translation.
pub struct Cx<'a> {
    pub module: &'a Module,
    pub scopes: Vec<Vec<(String, Binding)>>,
    pub names: HashSet<String>,
    pub vars: Vec<Option<IntK>>,
    pub temps: u32,
    pub errors: Vec<syn::Error>,
    pub ret: Ty,
    pub w: Writer,
    pub css: &'a mut Vec<(String, String)>,
    pub tokens: &'a mut Vec<String>,
    pub server_fns: &'a mut Vec<syn::Path>,
    pub self_ty: Option<String>,
    /// Inside a loop used as a value: the temporary its reak assigns.
    pub break_target: Option<Option<String>>,
    /// The type a value-reak gave.
    pub break_ty: Option<Ty>,
    /// Integer literals in rewritten arithmetic, with their kinds.
    pub lits: Vec<(String, IntK)>,
}

impl<'a> Cx<'a> {
    pub fn new(
        module: &'a Module,
        css: &'a mut Vec<(String, String)>,
        tokens: &'a mut Vec<String>,
        server_fns: &'a mut Vec<syn::Path>,
    ) -> Self {
        Self {
            module,
            scopes: vec![Vec::new()],
            names: HashSet::new(),
            vars: Vec::new(),
            temps: 0,
            errors: Vec::new(),
            ret: Ty::Unit,
            w: Writer::new(),
            css,
            tokens,
            server_fns,
            self_ty: None,
            break_target: None,
            break_ty: None,
            lits: Vec::new(),
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Binds `name` as a fresh JavaScript variable; returns its name.
    pub fn bind(&mut self, name: &str, ty: Ty) -> String {
        let base = ident(name);
        let mut js = base.clone();
        let mut counter = 1;
        while self.names.contains(&js) {
            js = format!("{base}${counter}");
            counter += 1;
        }
        self.names.insert(js.clone());
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_owned(), Binding { js: js.clone(), ty }));
        }
        js
    }

    /// Binds `name` to an existing JavaScript expression (an alias).
    pub fn alias(&mut self, name: &str, js: String, ty: Ty) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name.to_owned(), Binding { js, ty }));
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| {
            scope.iter().rev().find(|(bound, _)| bound == name).map(|(_, binding)| binding)
        })
    }

    /// Updates a binding's type (an integer literal's kind, learned later).
    pub fn temp(&mut self) -> String {
        self.temps += 1;
        let name = format!("$t{}", self.temps);
        self.names.insert(name.clone());
        name
    }

    pub fn fresh_int(&mut self) -> IntK {
        self.vars.push(None);
        IntK::Var(u32::try_from(self.vars.len() - 1).unwrap_or(0))
    }

    pub fn resolve(&self, kind: IntK) -> IntK {
        let mut kind = kind;
        for _ in 0..64 {
            match kind {
                IntK::Var(index) => match self.vars.get(index as usize).copied().flatten() {
                    Some(next) => kind = next,
                    None => return kind,
                },
                concrete => return concrete,
            }
        }
        kind
    }

    /// Makes two integer kinds one; `false` when they are different
    /// concrete kinds.
    pub fn unify_int(&mut self, a: IntK, b: IntK) -> bool {
        let (a, b) = (self.resolve(a), self.resolve(b));
        match (a, b) {
            (IntK::Var(x), IntK::Var(y)) if x == y => true,
            (IntK::Var(x), other) | (other, IntK::Var(x)) => {
                if let Some(slot) = self.vars.get_mut(x as usize) {
                    *slot = Some(other);
                }
                true
            }
            (a, b) => a == b,
        }
    }

    /// Makes two types one where they hold integer literals; returns the
    /// (resolved) type.
    pub fn unify(&mut self, a: &Ty, b: &Ty) -> Ty {
        match (a, b) {
            (Ty::Int(x), Ty::Int(y)) => {
                self.unify_int(*x, *y);
                Ty::Int(self.resolve(*x))
            }
            (Ty::Vec(x), Ty::Vec(y)) => Ty::Vec(Box::new(self.unify(x, y))),
            (Ty::Opt(x), Ty::Opt(y)) => Ty::Opt(Box::new(self.unify(x, y))),
            (Ty::Opaque(_), other) | (other, Ty::Opaque(_)) => other.clone(),
            (a, _) => a.clone(),
        }
    }

    /// An integer kind as a JavaScript string literal; an unresolved
    /// literal's kind is a placeholder resolved when the function ends.
    pub fn kind_js(&self, kind: IntK) -> String {
        match self.resolve(kind) {
            IntK::Var(index) => format!("\"\u{1}{index}\u{1}\""),
            concrete => format!("\"{}\"", concrete.name()),
        }
    }

    pub fn error(&mut self, error: syn::Error) {
        self.errors.push(error);
    }

    /// Resolves integer placeholders in the written lines.
    pub fn finish(self) -> (Writer, Vec<syn::Error>) {
        let Self { w, vars, errors, .. } = self;
        let resolve = |index: u32| {
            let mut kind = IntK::Var(index);
            for _ in 0..64 {
                match kind {
                    IntK::Var(index) => match vars.get(index as usize).copied().flatten() {
                        Some(next) => kind = next,
                        None => break,
                    },
                    _ => break,
                }
            }
            kind.name()
        };
        (
            w.map(|line| {
                let mut out = String::with_capacity(line.len());
                let mut parts = line.split('\u{1}');
                if let Some(first) = parts.next() {
                    out.push_str(first);
                }
                let mut inside = true;
                for part in parts {
                    if inside {
                        out.push_str(resolve(part.parse().unwrap_or(0)));
                    } else {
                        out.push_str(part);
                    }
                    inside = !inside;
                }
                out
            }),
            errors,
        )
    }
}

impl Writer {
    pub fn map(self, f: impl Fn(&str) -> String) -> Self {
        let (text, spans) = self.finish();
        let mut out = Self::new();
        for (line, span) in text.lines().zip(spans) {
            out.line(&f(line), span);
        }
        out
    }

    pub fn append(&mut self, other: Self) {
        let (text, spans) = other.finish();
        for (line, span) in text.lines().zip(spans) {
            self.line(line, span);
        }
    }
}

fn has_attr_path(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident(name))
}

fn derives(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("derive") {
            let _ = attr.parse_nested_meta(|meta| {
                out.push(crate::ty::last_segment(&meta.path));
                Ok(())
            });
        }
    }
    out
}

/// A `#[serde(...)]` attribute that changes the JSON shape is refused: the
/// generated code reads fields by their Rust names.
fn check_serde(attrs: &[syn::Attribute], errors: &mut Vec<syn::Error>) {
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| {
            let name = crate::ty::last_segment(&meta.path);
            if !matches!(name.as_str(), "default" | "deny_unknown_fields") {
                errors.push(refuse(
                    attr.span(),
                    &format!("`#[serde({name})]`"),
                    "it changes the JSON the state crosses to the browser in, which the generated code reads \
                     by the Rust names",
                ));
            }
            if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let _content;
                syn::parenthesized!(_content in meta.input);
            }
            Ok(())
        });
    }
}

fn collect(module: &syn::ItemMod, errors: &mut Vec<syn::Error>) -> Module {
    let mut out = Module { name: module.ident.to_string(), ..Module::default() };
    let Some((_, items)) = &module.content else { return out };
    // Names first, so field types can refer to types declared later.
    for item in items {
        match item {
            syn::Item::Struct(item) => {
                out.structs.insert(
                    item.ident.to_string(),
                    StructDef { fields: Vec::new(), tuple: false, copy: false, default: false },
                );
            }
            syn::Item::Enum(item) => {
                out.enums.insert(
                    item.ident.to_string(),
                    EnumDef { variants: Vec::new(), copy: false, default: None },
                );
            }
            _ => {}
        }
    }
    let names: HashSet<String> = out.structs.keys().chain(out.enums.keys()).cloned().collect();
    let local = |name: &str| names.contains(name);
    for item in items {
        match item {
            syn::Item::Struct(item) => {
                if !item.generics.params.is_empty() {
                    errors.push(refuse(
                        item.generics.span(),
                        "a generic type",
                        "client data has one JSON shape",
                    ));
                }
                check_serde(&item.attrs, errors);
                let derived = derives(&item.attrs);
                let tuple = matches!(item.fields, syn::Fields::Unnamed(_));
                let fields = item
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        check_serde(&field.attrs, errors);
                        let name = field
                            .ident
                            .as_ref()
                            .map_or_else(|| index.to_string(), ToString::to_string);
                        (name, read(&field.ty, &local))
                    })
                    .collect();
                out.structs.insert(
                    item.ident.to_string(),
                    StructDef {
                        fields,
                        tuple,
                        copy: derived.iter().any(|d| d == "Copy"),
                        default: derived.iter().any(|d| d == "Default"),
                    },
                );
            }
            syn::Item::Enum(item) => {
                if !item.generics.params.is_empty() {
                    errors.push(refuse(
                        item.generics.span(),
                        "a generic type",
                        "client data has one JSON shape",
                    ));
                }
                check_serde(&item.attrs, errors);
                let derived = derives(&item.attrs);
                let mut default = None;
                let variants = item
                    .variants
                    .iter()
                    .map(|variant| {
                        check_serde(&variant.attrs, errors);
                        if has_attr_path(&variant.attrs, "default") {
                            default = Some(variant.ident.to_string());
                        }
                        let shape = match &variant.fields {
                            syn::Fields::Unit => Shape::Unit,
                            syn::Fields::Unnamed(fields) => Shape::Tuple(
                                fields
                                    .unnamed
                                    .iter()
                                    .map(|field| read(&field.ty, &local))
                                    .collect(),
                            ),
                            syn::Fields::Named(fields) => Shape::Struct(
                                fields
                                    .named
                                    .iter()
                                    .map(|field| {
                                        (
                                            field
                                                .ident
                                                .as_ref()
                                                .map(ToString::to_string)
                                                .unwrap_or_default(),
                                            read(&field.ty, &local),
                                        )
                                    })
                                    .collect(),
                            ),
                        };
                        (variant.ident.to_string(), shape)
                    })
                    .collect();
                out.enums.insert(
                    item.ident.to_string(),
                    EnumDef { variants, copy: derived.iter().any(|d| d == "Copy"), default },
                );
            }
            _ => {}
        }
    }
    for item in items {
        match item {
            syn::Item::Fn(item) => {
                out.fns.insert(item.sig.ident.to_string(), signature(&item.sig, &local));
            }
            syn::Item::Const(item) => {
                out.consts.insert(item.ident.to_string(), read(&item.ty, &local));
            }
            syn::Item::Impl(item) => {
                let syn::Type::Path(self_ty) = &*item.self_ty else { continue };
                let type_name = crate::ty::last_segment(&self_ty.path);
                let trait_name =
                    item.trait_.as_ref().map(|(_, path, _)| crate::ty::last_segment(path));
                if trait_name.as_deref() == Some("Default") {
                    if let Some(def) = out.structs.get_mut(&type_name) {
                        def.default = true;
                    }
                }
                if trait_name.is_some() && trait_name.as_deref() != Some("Default") {
                    continue;
                }
                for impl_item in &item.items {
                    if let syn::ImplItem::Fn(function) = impl_item {
                        out.methods.insert(
                            (type_name.clone(), function.sig.ident.to_string()),
                            signature(&function.sig, &local),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn signature(sig: &syn::Signature, local: &dyn Fn(&str) -> bool) -> FnSig {
    let method = sig.receiver().is_some();
    let params = sig
        .inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Typed(typed) => Some(read(&typed.ty, local)),
            syn::FnArg::Receiver(_) => None,
        })
        .collect();
    let ret = match &sig.output {
        syn::ReturnType::Default => Ty::Unit,
        syn::ReturnType::Type(_, ty) => read(ty, local),
    };
    FnSig { params, ret, method }
}

/// Writes resolved literals in place of their placeholders.
struct Literals<'a>(&'a [String]);

impl syn::visit_mut::VisitMut for Literals<'_> {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        if let syn::Expr::Path(path) = expr {
            if let Some(index) = path.path.get_ident().and_then(|ident| {
                ident
                    .to_string()
                    .strip_prefix("__rn_lit_")
                    .and_then(|index| index.parse::<usize>().ok())
            }) {
                if let Some(literal) = self.0.get(index) {
                    let span = path.span();
                    if let Ok(parsed) = syn::parse_str::<syn::Expr>(literal) {
                        *expr = syn::parse_quote_spanned!(span=> #parsed);
                        return;
                    }
                }
            }
        }
        syn::visit_mut::visit_expr_mut(self, expr);
    }
}

/// Translates one function (or method of `self_ty`) into `w`.
fn function(
    module: &Module,
    function: &mut syn::ItemFn,
    js_name: &str,
    self_ty: Option<&str>,
    out: &mut Writer,
    shared: (&mut Vec<(String, String)>, &mut Vec<String>, &mut Vec<syn::Path>),
    errors: &mut Vec<syn::Error>,
) {
    let (css, tokens, server_fns) = shared;
    let mut cx = Cx::new(module, css, tokens, server_fns);
    cx.self_ty = self_ty.map(ToOwned::to_owned);
    if !function.sig.generics.params.is_empty() {
        cx.error(refuse(
            function.sig.generics.span(),
            "a generic function",
            "each call site's JavaScript is one function",
        ));
    }
    if function.sig.asyncness.is_some() {
        cx.error(refuse(
            function.sig.span(),
            "an `async fn`",
            "client logic requests asynchronous work through `fx`",
        ));
    }
    let mut params = Vec::new();
    for input in &function.sig.inputs {
        match input {
            syn::FnArg::Receiver(_) => {
                let ty = Ty::Adt(self_ty.unwrap_or_default().to_owned());
                cx.names.insert("self".into());
                cx.alias("self", "self".into(), ty);
                params.push("self".to_owned());
            }
            syn::FnArg::Typed(typed) => {
                let ty = module.read(&typed.ty);
                match &*typed.pat {
                    syn::Pat::Ident(pat) => params.push(cx.bind(&pat.ident.to_string(), ty)),
                    syn::Pat::Wild(_) => params.push(cx.temp()),
                    other => {
                        cx.error(refuse(
                            other.span(),
                            "a pattern parameter",
                            "name the parameter and destructure it in the body",
                        ));
                        params.push(cx.temp());
                    }
                }
            }
        }
    }
    cx.ret = match &function.sig.output {
        syn::ReturnType::Default => Ty::Unit,
        syn::ReturnType::Type(_, ty) => module.read(ty),
    };
    cx.w.open(
        &format!("function {js_name}({}) {{", params.join(", ")),
        Some(function.sig.ident.span()),
    );
    let ret = cx.ret.clone();
    cx.body(&mut function.block, &ret);
    cx.w.close("}");
    let literals: Vec<String> = cx
        .lits
        .iter()
        .map(|(digits, kind)| format!("{digits}{}", cx.resolve(*kind).name()))
        .collect();
    syn::visit_mut::VisitMut::visit_block_mut(&mut Literals(&literals), &mut function.block);
    let (written, mut refusals) = cx.finish();
    out.append(written);
    errors.append(&mut refusals);
}

pub fn translate_module(module: &mut syn::ItemMod) -> Result<Translation, Vec<syn::Error>> {
    let mut errors = Vec::new();
    let mut info = collect(module, &mut errors);
    let span = module.ident.span();
    // The state type: the one whose inherent impl has `view`.
    let state = info.methods.keys().find(|(_, method)| method == "view").map(|(ty, _)| ty.clone());
    let Some(state) = state else {
        return Err(vec![syn::Error::new(
            span,
            "a client module needs a state type with `fn view(&self) -> Node` and \
             `fn update(&mut self, event: Event, fx: &mut Effects<M>)` in an inherent impl",
        )]);
    };
    if !info.methods.contains_key(&(state.clone(), "update".to_owned())) {
        return Err(vec![syn::Error::new(
            span,
            format!("`{state}` needs `fn update(&mut self, event: Event, fx: &mut Effects<M>)`"),
        )]);
    }
    // The message type is `update`'s `Effects<M>`.
    let mut message_type: Option<syn::Type> = None;
    if let Some((_, items)) = &module.content {
        for item in items {
            if let syn::Item::Impl(item) = item {
                for impl_item in &item.items {
                    if let syn::ImplItem::Fn(function) = impl_item {
                        if function.sig.ident == "update" {
                            if let Some(syn::FnArg::Typed(typed)) =
                                function.sig.inputs.iter().nth(2)
                            {
                                if let syn::Type::Reference(reference) = &*typed.ty {
                                    if let syn::Type::Path(path) = &*reference.elem {
                                        if let Some(segment) = path.path.segments.last() {
                                            if let syn::PathArguments::AngleBracketed(args) =
                                                &segment.arguments
                                            {
                                                if let Some(syn::GenericArgument::Type(ty)) =
                                                    args.args.first()
                                                {
                                                    message_type = Some(ty.clone());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let Some(message_type) = message_type else {
        return Err(vec![syn::Error::new(
            span,
            format!("`{state}::update` takes `fx: &mut Effects<M>` as its third parameter"),
        )]);
    };
    info.message = info.read(&message_type);
    let has_message = info.methods.contains_key(&(state.clone(), "message".to_owned()));
    let has_init = info.methods.contains_key(&(state.clone(), "init".to_owned()));

    let mut out = Writer::new();
    let mut css = Vec::new();
    let mut tokens = Vec::new();
    let mut server_fns = Vec::new();
    out.line(
        &format!("// Generated by #[rustnative_web::client] from `{}`. Do not edit.", info.name),
        None,
    );
    out.open("export default function (rn, fns) {", None);
    out.line("\"use strict\";", None);
    // Defaults for the module's `Default` structs.
    let mut names: Vec<&String> = info.structs.keys().collect();
    names.sort();
    for name in names {
        let def = &info.structs[name];
        if !def.default {
            continue;
        }
        let manual = info.methods.contains_key(&(name.clone(), "default".to_owned()));
        if manual {
            continue;
        }
        let fields: Result<Vec<String>, syn::Error> = def
            .fields
            .iter()
            .map(|(field, ty)| {
                info.default_js(ty, span).map(|value| format!("{}: {value}", string(field)))
            })
            .collect();
        match fields {
            Ok(fields) if def.tuple && def.fields.len() == 1 => {
                let value =
                    fields[0].split_once(": ").map_or("null", |(_, value)| value).to_owned();
                out.line(&format!("function {name}$default() {{ return {value}; }}"), None);
            }
            Ok(fields) if def.tuple => {
                let values: Vec<&str> = fields
                    .iter()
                    .map(|field| field.split_once(": ").map_or("null", |(_, v)| v))
                    .collect();
                out.line(
                    &format!("function {name}$default() {{ return [{}]; }}", values.join(", ")),
                    None,
                );
            }
            Ok(fields) => out.line(
                &format!("function {name}$default() {{ return {{ {} }}; }}", fields.join(", ")),
                None,
            ),
            Err(error) => errors.push(error),
        }
    }
    let Some((_, items)) = &mut module.content else {
        return Err(vec![syn::Error::new(span, "`#[client]` needs an inline module")]);
    };
    for item in items.iter_mut() {
        match item {
            syn::Item::Const(item) => {
                let mut cx = Cx::new(&info, &mut css, &mut tokens, &mut server_fns);
                let ty = info.read(&item.ty);
                match cx.expr(&mut item.expr, Some(&ty)) {
                    Ok(value) if value.pre.is_empty() => {
                        let line =
                            format!("const {} = {};", ident(&item.ident.to_string()), value.js);
                        cx.w.line(&line, Some(item.ident.span()));
                    }
                    Ok(_) => cx.error(refuse(
                        item.expr.span(),
                        "this constant",
                        "a constant is one expression",
                    )),
                    Err(()) => {}
                }
                let (written, mut refusals) = cx.finish();
                out.append(written);
                errors.append(&mut refusals);
            }
            syn::Item::Fn(item) => {
                let name = ident(&item.sig.ident.to_string());
                function(
                    &info,
                    item,
                    &name,
                    None,
                    &mut out,
                    (&mut css, &mut tokens, &mut server_fns),
                    &mut errors,
                );
            }
            syn::Item::Impl(item) => {
                let syn::Type::Path(self_ty) = &*item.self_ty else { continue };
                let type_name = crate::ty::last_segment(&self_ty.path);
                let trait_name =
                    item.trait_.as_ref().map(|(_, path, _)| crate::ty::last_segment(path));
                if trait_name.is_some() && trait_name.as_deref() != Some("Default") {
                    continue;
                }
                for impl_item in &mut item.items {
                    let syn::ImplItem::Fn(method) = impl_item else { continue };
                    let mut function_item = syn::ItemFn {
                        attrs: Vec::new(),
                        vis: syn::Visibility::Inherited,
                        sig: method.sig.clone(),
                        block: Box::new(method.block.clone()),
                    };
                    let name = format!("{type_name}${}", method.sig.ident);
                    function(
                        &info,
                        &mut function_item,
                        &name,
                        Some(&type_name),
                        &mut out,
                        (&mut css, &mut tokens, &mut server_fns),
                        &mut errors,
                    );
                    method.block = *function_item.block;
                }
            }
            _ => {}
        }
    }
    let optional = |method: &str| {
        if info.methods.contains_key(&(state.clone(), method.to_owned())) {
            format!("{state}${method}")
        } else {
            "null".to_owned()
        }
    };
    out.line(
        &format!(
            "return {{ name: {}, update: {state}$update, message: {}, init: {}, view: {state}$view }};",
            string(&info.name),
            optional("message"),
            optional("init")
        ),
        None,
    );
    out.close("}");
    if !errors.is_empty() {
        return Err(errors);
    }
    let (js, lines) = out.finish();
    let mut seen = HashSet::new();
    css.retain(|(class, _)| seen.insert(class.clone()));
    tokens.sort();
    tokens.dedup();
    let mut unique_fns: Vec<syn::Path> = Vec::new();
    for path in server_fns {
        let name = crate::ty::last_segment(&path);
        if !unique_fns.iter().any(|existing| crate::ty::last_segment(existing) == name) {
            unique_fns.push(path);
        }
    }
    Ok(Translation {
        js,
        lines,
        css,
        tokens,
        server_fns: unique_fns,
        state: syn::Ident::new(&state, span),
        message: message_type,
        has_message,
        has_init,
    })
}
