//! Statements and expressions.
//!
//! Every expression translates to a [`V`]: statements that must run first
//! (`pre`), then a JavaScript expression. Control flow in value position —
//! an `if`, a `match`, a block, a `loop` that breaks with a value — becomes
//! statements assigning a temporary, so `return`, `break`, and `continue`
//! inside it mean what they mean in Rust (an arrow function around it would
//! change what they return from).
//!
//! The Rust is rewritten in the same walk: integer arithmetic becomes
//! `rt`'s checked arithmetic, and `rsx!` its builder expansion, so the Rust
//! that compiles is the Rust that was translated.

use std::fmt::Write as _;

use proc_macro2::Span;
use quote::ToTokens;
use syn::spanned::Spanned;

use crate::emit::{ident, string};
use crate::items::{Cx, Shape, V};
use crate::refuse;
use crate::ty::{IntK, Ty, last_segment};

pub type R<T> = Result<T, ()>;

const RT: &str = "::rustnative_web::client::rt";

/// `expr` without the parentheses around it, which a function argument
/// does not need (and `unused_parens` would flag in the user's crate).
fn bare(mut expr: &syn::Expr) -> &syn::Expr {
    while let syn::Expr::Paren(inner) = expr {
        expr = &inner.expr;
    }
    expr
}

fn rt_call(name: &str, span: Span, args: &[&syn::Expr]) -> syn::Expr {
    let name = syn::Ident::new(name, span);
    let args = args.iter().map(|arg| bare(arg));
    syn::parse_quote_spanned!(span=> ::rustnative_web::client::rt::#name(#(#args),*))
}

/// Flattens statements and a value: `pre` then `js`.
pub fn seq(pre: Vec<String>, js: impl Into<String>, ty: Ty) -> V {
    V { pre, js: js.into(), ty }
}

impl Cx<'_> {
    /// Records a refusal and fails the current expression.
    pub fn fail<T>(&mut self, span: Span, what: &str, why: &str) -> R<T> {
        self.error(refuse(span, what, why));
        Err(())
    }

    /// Writes `v`'s statements and returns its expression.
    pub fn flush(&mut self, v: V) -> String {
        for statement in v.pre {
            self.w.line(&statement, None);
        }
        v.js
    }

    /// Runs `f` writing into a fresh writer; returns what it wrote.
    pub fn capture(&mut self, f: impl FnOnce(&mut Self)) -> String {
        let saved = std::mem::take(&mut self.w);
        f(self);
        let written = std::mem::replace(&mut self.w, saved);
        let (text, _) = written.finish();
        text.trim_end().to_owned()
    }

    /// An unsuffixed integer literal operand, as a placeholder whose type
    /// suffix is written once the function's integer kinds are resolved
    /// (the checked-arithmetic functions take references too, so a bare
    /// literal would not infer); any other operand as written.
    pub fn literal_slot(&mut self, operand: &syn::Expr, kind: IntK) -> syn::Expr {
        let digits = match operand {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(int), .. })
                if int.suffix().is_empty() =>
            {
                int.base10_digits().to_owned()
            }
            syn::Expr::Unary(syn::ExprUnary { op: syn::UnOp::Neg(_), expr, .. }) => match &**expr {
                syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(int), .. })
                    if int.suffix().is_empty() =>
                {
                    format!("-{}", int.base10_digits())
                }
                _ => return operand.clone(),
            },
            syn::Expr::Paren(inner) => return self.literal_slot(&inner.expr, kind),
            _ => return operand.clone(),
        };
        self.lits.push((digits, kind));
        let name = syn::Ident::new(&format!("__rn_lit_{}", self.lits.len() - 1), operand.span());
        syn::parse_quote!(#name)
    }

    // ------------------------------------------------------ blocks ----

    /// A function body: its statements, and its tail as the return value.
    pub fn body(&mut self, block: &mut syn::Block, ret: &Ty) {
        tail_macro(block);
        self.push_scope();
        let count = block.stmts.len();
        for (index, statement) in block.stmts.iter_mut().enumerate() {
            let last = index + 1 == count;
            match statement {
                syn::Stmt::Expr(expr, None) if last => {
                    if *ret == Ty::Unit {
                        self.statement_expr(expr);
                    } else if let Ok(value) = self.expr(expr, Some(ret)) {
                        let span = expr.span();
                        let js = self.flush(value);
                        self.w.line(&format!("return {js};"), Some(span));
                    }
                }
                other => self.stmt(other),
            }
        }
        self.pop_scope();
    }

    /// A block's statements; its tail, if any, assigned to `target`.
    pub fn block_into(
        &mut self,
        block: &mut syn::Block,
        target: Option<&str>,
        expect: Option<&Ty>,
    ) -> Ty {
        tail_macro(block);
        self.push_scope();
        let count = block.stmts.len();
        let mut ty = Ty::Unit;
        for (index, statement) in block.stmts.iter_mut().enumerate() {
            let last = index + 1 == count;
            match statement {
                syn::Stmt::Expr(expr, None) if last => match target {
                    Some(target) => {
                        if let Ok(value) = self.expr(expr, expect) {
                            ty = value.ty.clone();
                            let span = expr.span();
                            let js = self.flush(value);
                            self.w.line(&format!("{target} = {js};"), Some(span));
                        }
                    }
                    None => self.statement_expr(expr),
                },
                other => self.stmt(other),
            }
        }
        self.pop_scope();
        ty
    }

    pub fn stmt(&mut self, statement: &mut syn::Stmt) {
        match statement {
            syn::Stmt::Local(local) => {
                let _ = self.local(local);
            }
            syn::Stmt::Expr(expr, _) => self.statement_expr(expr),
            syn::Stmt::Macro(mac) => {
                let span = mac.span();
                let mut expr = syn::Expr::Macro(syn::ExprMacro {
                    attrs: mac.attrs.clone(),
                    mac: mac.mac.clone(),
                });
                self.statement_expr(&mut expr);
                if let syn::Expr::Macro(rewritten) = expr {
                    mac.mac = rewritten.mac;
                } else {
                    // An `rsx!` expanded in statement position: keep the
                    // statement's value discarded.
                    *statement = syn::Stmt::Expr(expr, Some(syn::Token![;](span)));
                }
            }
            syn::Stmt::Item(item) => {
                self.error(refuse(
                    item.span(),
                    "an item inside a function",
                    "declare it in the module",
                ));
            }
        }
    }

    fn local(&mut self, local: &mut syn::Local) -> R<()> {
        let span = local.span();
        let (pat, declared) = match &local.pat {
            syn::Pat::Type(typed) => ((*typed.pat).clone(), Some(self.module.read(&typed.ty))),
            other => (other.clone(), None),
        };
        let Some(init) = &mut local.init else {
            // `let x;` assigned later.
            if let syn::Pat::Ident(ident) = &pat {
                let ty = declared.unwrap_or(Ty::Opaque("_".into()));
                let js = self.bind(&ident.ident.to_string(), ty);
                self.w.line(&format!("let {js};"), Some(span));
                return Ok(());
            }
            return self.fail(span, "a `let` without a value", "give it a value");
        };
        let value = self.expr(&mut init.expr, declared.as_ref())?;
        let ty = match &declared {
            Some(declared) => self.unify(declared, &value.ty),
            None => value.ty.clone(),
        };
        let copied = self.copy_if_place(&init.expr, value);
        let js = self.flush(copied);
        if let Some((_, diverge)) = &mut init.diverge {
            // `let PAT = value else { .. };`
            let scrutinee = self.temp();
            self.w.line(&format!("const {scrutinee} = {js};"), Some(span));
            let (condition, bindings) = self.pattern(&pat, &scrutinee, &ty)?;
            self.w.open(
                &format!("if (!({})) {{", condition.unwrap_or_else(|| "true".into())),
                Some(span),
            );
            self.push_scope();
            self.statement_expr(diverge);
            self.pop_scope();
            self.w.close("}");
            for (name, value, ty) in bindings {
                let variable = self.bind(&name, ty);
                self.w.line(&format!("let {variable} = {value};"), Some(span));
            }
            return Ok(());
        }
        if let syn::Pat::Ident(pat_ident) = &pat {
            if pat_ident.subpat.is_none() {
                let variable = self.bind(&pat_ident.ident.to_string(), ty);
                self.w.line(&format!("let {variable} = {js};"), Some(span));
                return Ok(());
            }
        }
        let scrutinee = self.temp();
        self.w.line(&format!("const {scrutinee} = {js};"), Some(span));
        let (_, bindings) = self.pattern(&pat, &scrutinee, &ty)?;
        for (name, value, ty) in bindings {
            let variable = self.bind(&name, ty);
            self.w.line(&format!("let {variable} = {value};"), Some(span));
        }
        Ok(())
    }

    /// A value read from a place of a `Copy` type is a copy in Rust; in
    /// JavaScript an object would be shared, so it is cloned.
    pub fn copy_if_place(&self, expr: &syn::Expr, value: V) -> V {
        let place = matches!(expr, syn::Expr::Path(_) | syn::Expr::Field(_) | syn::Expr::Index(_));
        if place && !value.ty.is_primitive() && self.module.is_copy(&value.ty) {
            return V { js: format!("rn.clone({})", value.js), ..value };
        }
        value
    }

    /// An expression in statement position.
    pub fn statement_expr(&mut self, expr: &mut syn::Expr) {
        let span = expr.span();
        let result: R<()> = match expr {
            syn::Expr::If(_) => self.if_expr(expr, None, None).map(|_| ()),
            syn::Expr::Match(_) => self.match_expr(expr, None, None).map(|_| ()),
            syn::Expr::Block(block) => {
                self.w.open("{", Some(span));
                self.block_into(&mut block.block, None, None);
                self.w.close("}");
                Ok(())
            }
            syn::Expr::Loop(_) | syn::Expr::While(_) | syn::Expr::ForLoop(_) => {
                self.loop_expr(expr, None).map(|_| ())
            }
            syn::Expr::Return(ret) => {
                let value = match &mut ret.expr {
                    Some(value) => {
                        let expect = self.ret.clone();
                        self.expr(value, Some(&expect)).map(Some)
                    }
                    None => Ok(None),
                };
                value.map(|value| match value {
                    Some(value) => {
                        let js = self.flush(value);
                        self.w.line(&format!("return {js};"), Some(span));
                    }
                    None => self.w.line("return;", Some(span)),
                })
            }
            syn::Expr::Break(brk) => {
                let label = brk
                    .label
                    .as_ref()
                    .map(|label| format!(" {}", label_js(label)))
                    .unwrap_or_default();
                if let Some(value) = &mut brk.expr {
                    match self.break_target.clone().flatten() {
                        Some(target) => self.expr(value, None).map(|value| {
                            let ty = value.ty.clone();
                            let js = self.flush(value);
                            self.break_ty = Some(ty);
                            self.w.line(&format!("{target} = {js};"), Some(span));
                            self.w.line(&format!("break{label};"), Some(span));
                        }),
                        None => self.fail(
                            span,
                            "`break` with a value here",
                            "only a `loop` used as a value breaks with one",
                        ),
                    }
                } else {
                    self.w.line(&format!("break{label};"), Some(span));
                    Ok(())
                }
            }
            syn::Expr::Continue(cont) => {
                let label = cont
                    .label
                    .as_ref()
                    .map(|label| format!(" {}", label_js(label)))
                    .unwrap_or_default();
                self.w.line(&format!("continue{label};"), Some(span));
                Ok(())
            }
            _ => self.expr(expr, None).map(|value| {
                let js = self.flush(value);
                if !js.is_empty() && js != "undefined" && js != "null" {
                    self.w.line(&format!("{js};"), Some(span));
                }
            }),
        };
        let _ = result;
    }

    // ------------------------------------------------- expressions ----

    #[allow(clippy::cognitive_complexity, reason = "one arm per expression kind")]
    pub fn expr(&mut self, expr: &mut syn::Expr, expect: Option<&Ty>) -> R<V> {
        let span = expr.span();
        match expr {
            syn::Expr::Lit(literal) => self.literal(&literal.lit, expect),
            syn::Expr::Paren(inner) => {
                let value = self.expr(&mut inner.expr, expect)?;
                Ok(V { js: format!("({})", value.js), ..value })
            }
            syn::Expr::Group(inner) => self.expr(&mut inner.expr, expect),
            syn::Expr::Reference(reference) => self.expr(&mut reference.expr, expect),
            syn::Expr::Path(path) => self.path_value(path, expect),
            syn::Expr::Unary(_) => self.unary(expr, expect),
            syn::Expr::Binary(_) => self.binary(expr, expect),
            syn::Expr::Assign(assign) => {
                let (pre, place, ty) = self.place(&mut assign.left)?;
                let value = self.expr(&mut assign.right, Some(&ty))?;
                self.unify(&ty, &value.ty);
                let value = self.copy_if_place(&assign.right, value);
                let mut all = pre;
                all.extend(value.pre);
                all.push(format!("{place} = {};", value.js));
                Ok(seq(all, "undefined", Ty::Unit))
            }
            syn::Expr::Field(field) => {
                let base = self.expr(&mut field.base, None)?;
                self.field(base, &field.member, span)
            }
            syn::Expr::Index(index) => {
                let base = self.expr(&mut index.expr, None)?;
                if matches!(*index.index, syn::Expr::Range(_)) {
                    return self.fail(span, "slicing", "the subset indexes collections one element at a time");
                }
                let position = self.expr(&mut index.index, Some(&Ty::Int(IntK::Usize)))?;
                let Some(element) = (match &base.ty {
                    Ty::Vec(inner) => Some((**inner).clone()),
                    Ty::Str => None,
                    _ => None,
                }) else {
                    return self.fail(span, &format!("indexing a {}", base.ty), "strings are indexed by bytes in Rust and by UTF-16 units in JavaScript");
                };
                let mut pre = base.pre;
                pre.extend(position.pre);
                Ok(seq(pre, format!("rn.at({}, {})", base.js, position.js), element))
            }
            syn::Expr::MethodCall(_) => self.method_call(expr, expect),
            syn::Expr::Call(_) => self.call(expr, expect),
            syn::Expr::Macro(_) => self.macro_expr(expr, expect),
            syn::Expr::If(_) => {
                let target = self.temp();
                let mut pre = vec![format!("let {target};")];
                let captured = self.capture_result(|cx| cx.if_expr(expr, Some(&target), expect));
                let (text, ty) = captured?;
                pre.push(text);
                Ok(seq(pre, target, ty))
            }
            syn::Expr::Match(_) => {
                let target = self.temp();
                let mut pre = vec![format!("let {target};")];
                let (text, ty) = self.capture_result(|cx| cx.match_expr(expr, Some(&target), expect))?;
                pre.push(text);
                Ok(seq(pre, target, ty))
            }
            syn::Expr::Block(block) => {
                let target = self.temp();
                let mut pre = vec![format!("let {target};")];
                let mut ty = Ty::Unit;
                let text = self.capture(|cx| {
                    cx.w.open("{", Some(span));
                    ty = cx.block_into(&mut block.block, Some(&target), expect);
                    cx.w.close("}");
                });
                pre.push(text);
                Ok(seq(pre, target, ty))
            }
            syn::Expr::Loop(_) => {
                let target = self.temp();
                let mut pre = vec![format!("let {target};")];
                let (text, ty) = self.capture_result(|cx| cx.loop_expr(expr, Some(&target)))?;
                pre.push(text);
                Ok(seq(pre, target, ty))
            }
            syn::Expr::While(_) | syn::Expr::ForLoop(_) | syn::Expr::Return(_) | syn::Expr::Break(_) | syn::Expr::Continue(_) => {
                let text = self.capture(|cx| cx.statement_expr(expr));
                Ok(seq(vec![text], "undefined", Ty::Unit))
            }
            syn::Expr::Tuple(tuple) => {
                if tuple.elems.is_empty() {
                    return Ok(V::new("null", Ty::Unit));
                }
                let expected: Vec<Option<Ty>> = match expect {
                    Some(Ty::Tuple(items)) => items.iter().cloned().map(Some).collect(),
                    _ => vec![None; tuple.elems.len()],
                };
                let mut pre = Vec::new();
                let mut items = Vec::new();
                let mut types = Vec::new();
                for (element, expected) in tuple.elems.iter_mut().zip(expected.into_iter().chain(std::iter::repeat(None))) {
                    let value = self.expr(element, expected.as_ref())?;
                    pre.extend(value.pre);
                    items.push(value.js);
                    types.push(value.ty);
                }
                Ok(seq(pre, format!("[{}]", items.join(", ")), Ty::Tuple(types)))
            }
            syn::Expr::Array(array) => {
                let element = match expect {
                    Some(Ty::Vec(inner)) => Some((**inner).clone()),
                    _ => None,
                };
                let mut pre = Vec::new();
                let mut items = Vec::new();
                let mut ty = element.clone();
                for item in &mut array.elems {
                    let value = self.expr(item, ty.as_ref())?;
                    pre.extend(value.pre);
                    items.push(value.js);
                    ty = Some(match ty {
                        Some(existing) => self.unify(&existing, &value.ty),
                        None => value.ty,
                    });
                }
                Ok(seq(pre, format!("[{}]", items.join(", ")), Ty::Vec(Box::new(ty.unwrap_or(Ty::Opaque("_".into()))))))
            }
            syn::Expr::Repeat(repeat) => {
                let value = self.expr(&mut repeat.expr, None)?;
                let count = self.expr(&mut repeat.len, Some(&Ty::Int(IntK::Usize)))?;
                let mut pre = value.pre;
                pre.extend(count.pre);
                Ok(seq(pre, format!("Array.from({{ length: {} }}, () => rn.clone({}))", count.js, value.js), Ty::Vec(Box::new(value.ty))))
            }
            syn::Expr::Closure(closure) => self.closure(closure, expect),
            syn::Expr::Struct(_) => self.struct_literal(expr, expect),
            syn::Expr::Cast(_) => self.cast(expr),
            syn::Expr::Range(range) => {
                let kind = match expect {
                    Some(Ty::Iter(inner)) | Some(Ty::Vec(inner)) => match **inner {
                        Ty::Int(kind) => kind,
                        _ => self.fresh_int(),
                    },
                    _ => self.fresh_int(),
                };
                let (Some(start), Some(end)) = (&mut range.start, &mut range.end) else {
                    return self.fail(span, "an open range", "give both ends");
                };
                let start = self.expr(start, Some(&Ty::Int(kind)))?;
                let end = self.expr(end, Some(&Ty::Int(kind)))?;
                self.unify(&start.ty, &end.ty);
                let kind = match start.ty {
                    Ty::Int(kind) => kind,
                    _ => kind,
                };
                let mut pre = start.pre;
                pre.extend(end.pre);
                let end_js = if matches!(range.limits, syn::RangeLimits::Closed(_)) {
                    format!("rn.add({}, 1, {})", end.js, self.kind_js(kind))
                } else {
                    end.js
                };
                Ok(seq(pre, format!("rn.range({}, {end_js})", start.js), Ty::Iter(Box::new(Ty::Int(kind)))))
            }
            syn::Expr::Let(_) => self.fail(span, "`let` outside an `if` or `while` condition", "bind with `let` in a statement"),
            syn::Expr::Try(_) => self.fail(span, "the `?` operator", "match on the `Option` or `Result`"),
            syn::Expr::Async(_) | syn::Expr::Await(_) => {
                self.fail(span, "asynchronous code", "client logic requests asynchronous work through `fx` and handles the answer in `message`")
            }
            syn::Expr::Unsafe(_) => self.fail(span, "`unsafe`", "there is no memory to reach in the browser"),
            _ => self.fail(span, "this expression", "the subset does not translate it"),
        }
    }

    /// Like [`Self::capture`], for a step that returns a type.
    pub fn capture_result(&mut self, f: impl FnOnce(&mut Self) -> R<Ty>) -> R<(String, Ty)> {
        let mut result = Err(());
        let text = self.capture(|cx| result = f(cx));
        result.map(|ty| (text, ty))
    }

    fn literal(&mut self, literal: &syn::Lit, expect: Option<&Ty>) -> R<V> {
        match literal {
            syn::Lit::Int(int) => {
                let kind = if int.suffix().is_empty() {
                    match expect {
                        Some(Ty::Int(kind)) => *kind,
                        Some(Ty::Float(f32)) => {
                            return Ok(V::new(int.base10_digits(), Ty::Float(*f32)));
                        }
                        _ => self.fresh_int(),
                    }
                } else if let Some(kind) = IntK::from_name(int.suffix()) {
                    kind
                } else if int.suffix() == "f64" || int.suffix() == "f32" {
                    return Ok(V::new(int.base10_digits(), Ty::Float(int.suffix() == "f32")));
                } else {
                    return self.fail(int.span(), "this literal", "its type is not in the subset");
                };
                let digits = int.base10_digits();
                if digits.len() > 15
                    && digits.parse::<u64>().is_ok_and(|value| value > 9_007_199_254_740_991)
                {
                    return self.fail(
                        int.span(),
                        "this literal",
                        "it is beyond ±(2^53 − 1), which JavaScript cannot represent exactly",
                    );
                }
                Ok(V::new(digits, Ty::Int(kind)))
            }
            syn::Lit::Float(float) => {
                let f32 = float.suffix() == "f32" || matches!(expect, Some(Ty::Float(true)));
                let digits = float.base10_digits();
                Ok(V::new(
                    if f32 { format!("Math.fround({digits})") } else { digits.to_owned() },
                    Ty::Float(f32),
                ))
            }
            syn::Lit::Str(text) => Ok(V::new(string(&text.value()), Ty::Str)),
            syn::Lit::Char(ch) => Ok(V::new(string(&ch.value().to_string()), Ty::Char)),
            syn::Lit::Bool(value) => {
                Ok(V::new(if value.value { "true" } else { "false" }, Ty::Bool))
            }
            other => self.fail(other.span(), "this literal", "byte strings are not client data"),
        }
    }

    // ------------------------------------------------------ places ----

    /// An assignable place: its statements, its JavaScript, its type.
    pub fn place(&mut self, expr: &mut syn::Expr) -> R<(Vec<String>, String, Ty)> {
        let span = expr.span();
        match expr {
            syn::Expr::Paren(inner) => self.place(&mut inner.expr),
            syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.place(&mut unary.expr)
            }
            syn::Expr::Path(path) if path.path.segments.len() == 1 => {
                let name = path.path.segments[0].ident.to_string();
                match self.lookup(&name) {
                    Some(binding) => Ok((Vec::new(), binding.js.clone(), binding.ty.clone())),
                    None => self.fail(
                        span,
                        &format!("`{name}`"),
                        "it is not a variable of this function",
                    ),
                }
            }
            syn::Expr::Field(field) => {
                let (pre, base, ty) = self.place(&mut field.base)?;
                let value = self.field(seq(pre, base, ty), &field.member, span)?;
                Ok((value.pre, value.js, value.ty))
            }
            syn::Expr::Index(index) => {
                let (mut pre, base, ty) = self.place(&mut index.expr)?;
                let position = self.expr(&mut index.index, Some(&Ty::Int(IntK::Usize)))?;
                pre.extend(position.pre);
                let Ty::Vec(element) = ty else {
                    return self.fail(span, "this index", "only a `Vec` is indexed in the subset");
                };
                Ok((pre, format!("{base}[rn.idx({base}, {})]", position.js), *element))
            }
            _ => {
                self.fail(span, "assigning to this", "assign to a variable, a field, or an element")
            }
        }
    }

    pub fn field(&mut self, base: V, member: &syn::Member, span: Span) -> R<V> {
        let V { pre, js, ty } = base;
        match (member, &ty) {
            (syn::Member::Named(name), Ty::Adt(adt)) => {
                let name = name.to_string();
                let Some(def) = self.module.structs.get(adt) else {
                    return self.fail(span, &format!("`.{name}`"), "only a struct has fields");
                };
                let Some((_, field_ty)) = def.fields.iter().find(|(field, _)| *field == name)
                else {
                    return self.fail(
                        span,
                        &format!("`.{name}`"),
                        &format!("`{adt}` has no such field"),
                    );
                };
                let field_ty = field_ty.clone();
                Ok(seq(pre, format!("{js}.{}", ident_field(&name)), field_ty))
            }
            (syn::Member::Unnamed(index), Ty::Adt(adt)) => {
                let def = self.module.structs.get(adt).cloned();
                let Some(def) = def else {
                    return self.fail(span, "a tuple field", "only a struct has fields");
                };
                let index = index.index as usize;
                let Some((_, field_ty)) = def.fields.get(index) else {
                    return self.fail(span, "this field", "no such field");
                };
                // A one-field tuple struct is its field in JSON.
                if def.fields.len() == 1 {
                    return Ok(seq(pre, js, field_ty.clone()));
                }
                Ok(seq(pre, format!("{js}[{index}]"), field_ty.clone()))
            }
            (syn::Member::Unnamed(index), Ty::Tuple(items)) => {
                let index = index.index as usize;
                let Some(item) = items.get(index) else {
                    return self.fail(span, "this field", "no such element");
                };
                Ok(seq(pre, format!("{js}[{index}]"), item.clone()))
            }
            (syn::Member::Named(name), framework) => {
                let name = name.to_string();
                let field_ty = crate::methods::framework_field(framework, &name);
                match field_ty {
                    Some(field_ty) => Ok(seq(pre, format!("{js}.{name}"), field_ty)),
                    None => self.fail(
                        span,
                        &format!("`.{name}` on {ty}"),
                        "the subset does not know that field",
                    ),
                }
            }
            _ => self.fail(span, "this field", &format!("{ty} has no fields the subset knows")),
        }
    }

    // --------------------------------------------------- operators ----

    fn unary(&mut self, expr: &mut syn::Expr, expect: Option<&Ty>) -> R<V> {
        let syn::Expr::Unary(unary) = expr else { return Err(()) };
        let span = unary.span();
        match unary.op {
            syn::UnOp::Deref(_) => self.expr(&mut unary.expr, expect),
            syn::UnOp::Not(_) => {
                let value = self.expr(&mut unary.expr, Some(&Ty::Bool))?;
                if value.ty != Ty::Bool {
                    return self.fail(
                        span,
                        "`!` on an integer",
                        "bitwise operators are not in the subset",
                    );
                }
                Ok(V { js: format!("!{}", paren(&value.js)), ..value })
            }
            syn::UnOp::Neg(_) => {
                // A negative literal is a literal.
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(_) | syn::Lit::Float(_),
                    ..
                }) = &*unary.expr
                {
                    let value = self.expr(&mut unary.expr, expect)?;
                    return Ok(V { js: format!("-{}", value.js), ..value });
                }
                let value = self.expr(&mut unary.expr, expect)?;
                match value.ty.clone() {
                    Ty::Int(kind) => {
                        let js = format!("rn.neg({}, {})", value.js, self.kind_js(kind));
                        let operand = (*unary.expr).clone();
                        let operand = self.literal_slot(&operand, kind);
                        *expr = rt_call("neg", span, &[&operand]);
                        Ok(seq(value.pre, js, Ty::Int(kind)))
                    }
                    Ty::Float(f32) => {
                        Ok(seq(value.pre, format!("(-{})", paren(&value.js)), Ty::Float(f32)))
                    }
                    other => {
                        self.fail(span, &format!("`-` on {other}"), "only numbers are negated")
                    }
                }
            }
            _ => self.fail(span, "this operator", "not in the subset"),
        }
    }

    fn binary(&mut self, expr: &mut syn::Expr, expect: Option<&Ty>) -> R<V> {
        let syn::Expr::Binary(binary) = expr else { return Err(()) };
        let span = binary.span();
        use syn::BinOp as B;
        let op = binary.op;
        // Short-circuiting operators keep their right side conditional.
        if matches!(op, B::And(_) | B::Or(_)) {
            let left = self.expr(&mut binary.left, Some(&Ty::Bool))?;
            let right = self.expr(&mut binary.right, Some(&Ty::Bool))?;
            let symbol = if matches!(op, B::And(_)) { "&&" } else { "||" };
            if right.pre.is_empty() {
                return Ok(seq(left.pre, format!("({} {symbol} {})", left.js, right.js), Ty::Bool));
            }
            let target = self.temp();
            let mut pre = left.pre;
            pre.push(format!("let {target} = {};", left.js));
            let condition = if symbol == "&&" { target.clone() } else { format!("!{target}") };
            let mut inner = right.pre.join("\n");
            let _ = write!(inner, "\n{target} = {};", right.js);
            pre.push(format!("if ({condition}) {{\n{inner}\n}}"));
            return Ok(seq(pre, target, Ty::Bool));
        }
        let compound = matches!(
            op,
            B::AddAssign(_) | B::SubAssign(_) | B::MulAssign(_) | B::DivAssign(_) | B::RemAssign(_)
        );
        if compound {
            let (pre, place, ty) = self.place(&mut binary.left)?;
            let right = self.expr(&mut binary.right, Some(&ty))?;
            let ty = self.unify(&ty, &right.ty);
            let mut all = pre;
            all.extend(right.pre);
            let name = match op {
                B::AddAssign(_) => "add",
                B::SubAssign(_) => "sub",
                B::MulAssign(_) => "mul",
                B::DivAssign(_) => "div",
                _ => "rem",
            };
            let js = match &ty {
                Ty::Int(kind) => {
                    let left = (*binary.left).clone();
                    let rhs = self.literal_slot(&binary.right, *kind);
                    let call = rt_call(name, span, &[&left, &rhs]);
                    *expr = syn::parse_quote_spanned!(span=> #left = #call);
                    format!("{place} = rn.{name}({place}, {}, {})", right.js, self.kind_js(*kind))
                }
                Ty::Float(f32) => {
                    let symbol = match name {
                        "add" => "+",
                        "sub" => "-",
                        "mul" => "*",
                        "div" => "/",
                        _ => "%",
                    };
                    if *f32 {
                        format!("{place} = Math.fround({place} {symbol} {})", right.js)
                    } else {
                        format!("{place} = {place} {symbol} {}", right.js)
                    }
                }
                Ty::Str if name == "add" => format!("{place} = {place} + {}", right.js),
                other => {
                    return self.fail(
                        span,
                        &format!("`{}` on {other}", op.to_token_stream()),
                        "only numbers and strings",
                    );
                }
            };
            all.push(format!("{js};"));
            return Ok(seq(all, "undefined", Ty::Unit));
        }
        let arithmetic = matches!(op, B::Add(_) | B::Sub(_) | B::Mul(_) | B::Div(_) | B::Rem(_));
        let left_expect = if arithmetic { expect.cloned() } else { None };
        let left = self.expr(&mut binary.left, left_expect.as_ref())?;
        let right_expect = left.ty.clone();
        let right = self.expr(&mut binary.right, Some(&right_expect))?;
        let ty = self.unify(&left.ty, &right.ty);
        let mut pre = left.pre.clone();
        pre.extend(right.pre.clone());
        let (a, b) = (left.js.clone(), right.js.clone());
        if arithmetic {
            let name = match op {
                B::Add(_) => "add",
                B::Sub(_) => "sub",
                B::Mul(_) => "mul",
                B::Div(_) => "div",
                _ => "rem",
            };
            return match &ty {
                Ty::Int(kind) => {
                    let js = format!("rn.{name}({a}, {b}, {})", self.kind_js(*kind));
                    let (l, r) = (
                        self.literal_slot(&binary.left, *kind),
                        self.literal_slot(&binary.right, *kind),
                    );
                    *expr = rt_call(name, span, &[&l, &r]);
                    Ok(seq(pre, js, Ty::Int(*kind)))
                }
                Ty::Float(f32) => {
                    let symbol = match name {
                        "add" => "+",
                        "sub" => "-",
                        "mul" => "*",
                        "div" => "/",
                        _ => "%",
                    };
                    let js = format!("({a} {symbol} {b})");
                    Ok(seq(
                        pre,
                        if *f32 { format!("Math.fround{js}") } else { js },
                        Ty::Float(*f32),
                    ))
                }
                Ty::Str if name == "add" => Ok(seq(pre, format!("({a} + {b})"), Ty::Str)),
                other => self.fail(
                    span,
                    &format!("`{}` on {other}", op.to_token_stream()),
                    "arithmetic is on numbers, `+` also on strings",
                ),
            };
        }
        let comparison = match op {
            B::Eq(_) => "==",
            B::Ne(_) => "!=",
            B::Lt(_) => "<",
            B::Le(_) => "<=",
            B::Gt(_) => ">",
            B::Ge(_) => ">=",
            _ => {
                return self.fail(
                    span,
                    &format!("`{}`", op.to_token_stream()),
                    "bitwise operators and shifts are not in the subset",
                );
            }
        };
        let numeric = ty.is_numeric() || matches!(ty, Ty::Bool);
        let js = match comparison {
            "==" | "!=" => {
                let negate = comparison == "!=";
                if ty.is_primitive() {
                    format!("({a} {} {b})", if negate { "!==" } else { "===" })
                } else {
                    format!("{}rn.eq({a}, {b})", if negate { "!" } else { "" })
                }
            }
            order if numeric => format!("({a} {order} {b})"),
            order => format!("(rn.cmp({a}, {b}) {order} 0)"),
        };
        Ok(seq(pre, js, Ty::Bool))
    }

    fn cast(&mut self, expr: &mut syn::Expr) -> R<V> {
        let syn::Expr::Cast(cast) = expr else { return Err(()) };
        let span = cast.span();
        let target = self.module.read(&cast.ty);
        let value = self.expr(&mut cast.expr, None)?;
        match (&value.ty, &target) {
            (from, Ty::Int(kind)) => {
                let kind = *kind;
                let js = format!("rn.castInt({}, {})", value.js, self.kind_js(kind));
                let operand = bare(&cast.expr).clone();
                let ty = &cast.ty;
                let path: syn::Path = syn::parse_str(&format!("{RT}::cast")).map_err(|_| ())?;
                let path_float: syn::Path =
                    syn::parse_str(&format!("{RT}::cast_float")).map_err(|_| ())?;
                // Any other source (`bool`, `char`) stays an `as` cast.
                match from {
                    Ty::Int(_) => {
                        *expr = syn::parse_quote_spanned!(span=> #path::<_, #ty>(#operand))
                    }
                    Ty::Float(true) => {
                        *expr = syn::parse_quote_spanned!(span=> #path_float::<#ty>(f64::from(#operand)))
                    }
                    Ty::Float(false) => {
                        *expr = syn::parse_quote_spanned!(span=> #path_float::<#ty>(#operand))
                    }
                    _ => {}
                }
                Ok(seq(value.pre, js, Ty::Int(kind)))
            }
            (_, Ty::Float(f32)) => {
                let js = format!(
                    "rn.castFloat({}, {})",
                    value.js,
                    if *f32 { "\"f32\"" } else { "\"f64\"" }
                );
                Ok(seq(value.pre, js, Ty::Float(*f32)))
            }
            (Ty::Int(_), Ty::Char) => {
                Ok(seq(value.pre, format!("rn.charFromU8({})", value.js), Ty::Char))
            }
            (from, to) => self.fail(
                span,
                &format!("`{from} as {to}`"),
                "the subset casts between numbers, and from `u8` to `char`",
            ),
        }
    }

    // ------------------------------------------------ control flow ----

    /// An `if`, writing its branches; each branch's value goes to
    /// `target` when there is one.
    pub fn if_expr(
        &mut self,
        expr: &mut syn::Expr,
        target: Option<&str>,
        expect: Option<&Ty>,
    ) -> R<Ty> {
        let syn::Expr::If(if_expr) = expr else { return Err(()) };
        let span = if_expr.span();
        let has_let = contains_let(&if_expr.cond);
        let mut ty;
        if has_let {
            // `if let` (and chains of them): bind inside, remember whether
            // it matched, and run `else` when it did not.
            let matched = self.temp();
            self.w.line(&format!("let {matched} = false;"), Some(span));
            self.w.open("{", Some(span));
            self.push_scope();
            let depth = self.condition_chain(&mut if_expr.cond)?;
            self.w.line(&format!("{matched} = true;"), None);
            ty = self.block_into(&mut if_expr.then_branch, target, expect);
            for _ in 0..depth {
                self.w.close("}");
            }
            self.pop_scope();
            self.w.close("}");
            if let Some((_, otherwise)) = &mut if_expr.else_branch {
                self.w.open(&format!("if (!{matched}) {{"), None);
                let other = self.else_branch(otherwise, target, expect)?;
                if ty == Ty::Unit {
                    ty = other;
                }
                self.w.close("}");
            }
            return Ok(ty);
        }
        let condition = self.expr(&mut if_expr.cond, Some(&Ty::Bool))?;
        let js = self.flush(condition);
        self.w.open(&format!("if ({js}) {{"), Some(span));
        ty = self.block_into(&mut if_expr.then_branch, target, expect);
        if let Some((_, otherwise)) = &mut if_expr.else_branch {
            self.w.close("} else {");
            self.w.open("", None);
            let other = self.else_branch(otherwise, target, expect)?;
            if ty == Ty::Unit {
                ty = other;
            }
        }
        self.w.close("}");
        Ok(ty)
    }

    fn else_branch(
        &mut self,
        otherwise: &mut syn::Expr,
        target: Option<&str>,
        expect: Option<&Ty>,
    ) -> R<Ty> {
        match otherwise {
            syn::Expr::Block(block) => Ok(self.block_into(&mut block.block, target, expect)),
            syn::Expr::If(_) => self.if_expr(otherwise, target, expect),
            other => {
                let value = self.expr(other, expect)?;
                let ty = value.ty.clone();
                let js = self.flush(value);
                if let Some(target) = target {
                    self.w.line(&format!("{target} = {js};"), None);
                }
                Ok(ty)
            }
        }
    }

    /// Writes nested `if`s for a condition of `let`s and booleans joined by
    /// `&&`; returns how many it opened.
    fn condition_chain(&mut self, condition: &mut syn::Expr) -> R<usize> {
        match condition {
            syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
                let left = self.condition_chain(&mut binary.left)?;
                let right = self.condition_chain(&mut binary.right)?;
                Ok(left + right)
            }
            syn::Expr::Let(pattern) => {
                let span = pattern.span();
                let value = self.expr(&mut pattern.expr, None)?;
                let ty = value.ty.clone();
                let js = self.flush(value);
                let scrutinee = self.temp();
                self.w.line(&format!("const {scrutinee} = {js};"), Some(span));
                let (test, bindings) = self.pattern(&pattern.pat, &scrutinee, &ty)?;
                self.w.open(
                    &format!("if ({}) {{", test.unwrap_or_else(|| "true".into())),
                    Some(span),
                );
                for (name, value, ty) in bindings {
                    let variable = self.bind(&name, ty);
                    self.w.line(&format!("let {variable} = {value};"), None);
                }
                Ok(1)
            }
            other => {
                let value = self.expr(other, Some(&Ty::Bool))?;
                let js = self.flush(value);
                self.w.open(&format!("if ({js}) {{"), None);
                Ok(1)
            }
        }
    }

    pub fn match_expr(
        &mut self,
        expr: &mut syn::Expr,
        target: Option<&str>,
        expect: Option<&Ty>,
    ) -> R<Ty> {
        let syn::Expr::Match(match_expr) = expr else { return Err(()) };
        let span = match_expr.span();
        let value = self.expr(&mut match_expr.expr, None)?;
        let ty = value.ty.clone();
        let js = self.flush(value);
        let scrutinee = self.temp();
        let label = format!("$m{}", self.temps);
        self.w.line(&format!("const {scrutinee} = {js};"), Some(span));
        self.w.open(&format!("{label}: {{"), Some(span));
        let mut result = Ty::Unit;
        for arm in &mut match_expr.arms {
            let arm_span = arm.pat.span();
            self.push_scope();
            let (test, bindings) = match self.pattern(&arm.pat, &scrutinee, &ty) {
                Ok(pattern) => pattern,
                Err(()) => {
                    self.pop_scope();
                    continue;
                }
            };
            self.w.open(
                &format!("if ({}) {{", test.unwrap_or_else(|| "true".into())),
                Some(arm_span),
            );
            for (name, value, ty) in bindings {
                let variable = self.bind(&name, ty);
                self.w.line(&format!("let {variable} = {value};"), None);
            }
            let mut guard_open = false;
            if let Some((_, guard)) = &mut arm.guard {
                if let Ok(guard) = self.expr(guard, Some(&Ty::Bool)) {
                    let js = self.flush(guard);
                    self.w.open(&format!("if ({js}) {{"), None);
                    guard_open = true;
                }
            }
            let arm_ty = match &mut *arm.body {
                syn::Expr::Block(block) => self.block_into(
                    &mut block.block,
                    target,
                    expect.or(Some(&result)).filter(|ty| **ty != Ty::Unit),
                ),
                body => {
                    let expected =
                        expect.cloned().or_else(|| (result != Ty::Unit).then(|| result.clone()));
                    match target {
                        Some(target) => match self.expr(body, expected.as_ref()) {
                            Ok(value) => {
                                let ty = value.ty.clone();
                                let js = self.flush(value);
                                self.w.line(&format!("{target} = {js};"), None);
                                ty
                            }
                            Err(()) => Ty::Unit,
                        },
                        None => {
                            self.statement_expr(body);
                            Ty::Unit
                        }
                    }
                }
            };
            if result == Ty::Unit || matches!(result, Ty::Opaque(_)) {
                result = arm_ty;
            } else {
                result = self.unify(&result, &arm_ty);
            }
            self.w.line(&format!("break {label};"), None);
            if guard_open {
                self.w.close("}");
            }
            self.w.close("}");
            self.pop_scope();
        }
        self.w.line("rn.panic(\"no match arm matched\");", None);
        self.w.close("}");
        Ok(result)
    }

    pub fn loop_expr(&mut self, expr: &mut syn::Expr, target: Option<&str>) -> R<Ty> {
        let span = expr.span();
        match expr {
            syn::Expr::Loop(looped) => {
                let label = looped
                    .label
                    .as_ref()
                    .map(|label| format!("{}: ", label_js(&label.name)))
                    .unwrap_or_default();
                self.w.open(&format!("{label}while (true) {{"), Some(span));
                let saved = self.break_target.replace(target.map(ToOwned::to_owned));
                self.block_into(&mut looped.body, None, None);
                self.break_target = saved;
                self.w.close("}");
                Ok(self.break_ty.take().unwrap_or(Ty::Unit))
            }
            syn::Expr::While(looped) => {
                let label = looped
                    .label
                    .as_ref()
                    .map(|label| format!("{}: ", label_js(&label.name)))
                    .unwrap_or_default();
                if contains_let(&looped.cond) {
                    self.w.open(&format!("{label}while (true) {{"), Some(span));
                    self.push_scope();
                    let depth = self.condition_chain(&mut looped.cond)?;
                    self.block_into(&mut looped.body, None, None);
                    self.w.line("continue;", None);
                    for _ in 0..depth {
                        self.w.close("}");
                    }
                    self.w.line("break;", None);
                    self.pop_scope();
                    self.w.close("}");
                    return Ok(Ty::Unit);
                }
                self.w.open(&format!("{label}while (true) {{"), Some(span));
                let condition = self.expr(&mut looped.cond, Some(&Ty::Bool))?;
                let js = self.flush(condition);
                self.w.line(&format!("if (!({js})) break;"), None);
                self.block_into(&mut looped.body, None, None);
                self.w.close("}");
                Ok(Ty::Unit)
            }
            syn::Expr::ForLoop(looped) => {
                let label = looped
                    .label
                    .as_ref()
                    .map(|label| format!("{}: ", label_js(&label.name)))
                    .unwrap_or_default();
                // A range loops by counting.
                if let syn::Expr::Range(range) = &mut *looped.expr {
                    if let (Some(start), Some(end), syn::Pat::Ident(pat)) =
                        (&mut range.start, &mut range.end, &*looped.pat)
                    {
                        let kind = self.fresh_int();
                        let start = self.expr(start, Some(&Ty::Int(kind)))?;
                        let end = self.expr(end, Some(&Ty::Int(kind)))?;
                        let ty = self.unify(&start.ty, &end.ty);
                        let start = self.flush(start);
                        let end = self.flush(end);
                        let bound = self.temp();
                        let comparison = if matches!(range.limits, syn::RangeLimits::Closed(_)) {
                            "<="
                        } else {
                            "<"
                        };
                        self.push_scope();
                        let variable = self.bind(&pat.ident.to_string(), ty);
                        self.w.open(&format!("{label}for (let {variable} = {start}, {bound} = {end}; {variable} {comparison} {bound}; {variable}++) {{"), Some(span));
                        self.block_into(&mut looped.body, None, None);
                        self.w.close("}");
                        self.pop_scope();
                        return Ok(Ty::Unit);
                    }
                }
                // `for x in v.iter_mut()` over primitives: `x` is `v[i]`.
                if let syn::Expr::MethodCall(call) = &mut *looped.expr {
                    if call.method == "iter_mut" {
                        let receiver = self.expr(&mut call.receiver, None)?;
                        if let (Ty::Vec(element), syn::Pat::Ident(pat)) =
                            (&receiver.ty, &*looped.pat)
                        {
                            if element.is_primitive() {
                                let element = (**element).clone();
                                let array = self.flush(receiver);
                                let index = self.temp();
                                self.push_scope();
                                self.alias(
                                    &pat.ident.to_string(),
                                    format!("{array}[{index}]"),
                                    element,
                                );
                                self.w.open(&format!("{label}for (let {index} = 0; {index} < {array}.length; {index}++) {{"), Some(span));
                                self.block_into(&mut looped.body, None, None);
                                self.w.close("}");
                                self.pop_scope();
                                return Ok(Ty::Unit);
                            }
                        }
                        let value = V::new(receiver.js.clone(), receiver.ty.clone());
                        return self.for_each(
                            looped,
                            seq(receiver.pre, value.js, value.ty),
                            &label,
                            span,
                        );
                    }
                }
                let iterable = self.expr(&mut looped.expr, None)?;
                self.for_each(looped, iterable, &label, span)
            }
            _ => Err(()),
        }
    }

    fn for_each(
        &mut self,
        looped: &mut syn::ExprForLoop,
        iterable: V,
        label: &str,
        span: Span,
    ) -> R<Ty> {
        let Some(element) = iterable.ty.element() else {
            return self.fail(
                span,
                &format!("iterating {}", iterable.ty),
                "the subset iterates vectors, iterators, ranges, and strings' characters",
            );
        };
        let js = self.flush(iterable);
        let item = self.temp();
        self.push_scope();
        self.w.open(&format!("{label}for (const {item} of {js}) {{"), Some(span));
        let (_, bindings) = self.pattern(&looped.pat, &item, &element)?;
        for (name, value, ty) in bindings {
            let variable = self.bind(&name, ty);
            self.w.line(&format!("let {variable} = {value};"), None);
        }
        self.block_into(&mut looped.body, None, None);
        self.w.close("}");
        self.pop_scope();
        Ok(Ty::Unit)
    }

    // ---------------------------------------------------- closures ----

    pub fn closure(&mut self, closure: &mut syn::ExprClosure, expect: Option<&Ty>) -> R<V> {
        let span = closure.span();
        let params: Vec<Ty> = match expect {
            Some(Ty::Fn(_)) | None => vec![Ty::Opaque("_".into()); closure.inputs.len()],
            Some(Ty::Tuple(items)) => items.clone(),
            Some(other) => vec![other.clone(); closure.inputs.len()],
        };
        self.closure_with(closure, &params, span)
    }

    /// A closure whose parameters have `params`' types.
    pub fn closure_with(
        &mut self,
        closure: &mut syn::ExprClosure,
        params: &[Ty],
        span: Span,
    ) -> R<V> {
        self.push_scope();
        let mut names = Vec::new();
        let mut destructure = Vec::new();
        for (input, ty) in closure
            .inputs
            .iter()
            .zip(params.iter().cloned().chain(std::iter::repeat(Ty::Opaque("_".into()))))
        {
            let (pat, ty) = match input {
                syn::Pat::Type(typed) => ((*typed.pat).clone(), self.module.read(&typed.ty)),
                other => (other.clone(), ty),
            };
            match &pat {
                syn::Pat::Ident(ident) => names.push(self.bind(&ident.ident.to_string(), ty)),
                syn::Pat::Wild(_) => names.push(self.temp()),
                syn::Pat::Reference(reference) if matches!(&*reference.pat, syn::Pat::Ident(_)) => {
                    let syn::Pat::Ident(ident) = &*reference.pat else { unreachable!() };
                    names.push(self.bind(&ident.ident.to_string(), ty));
                }
                other => {
                    let argument = self.temp();
                    names.push(argument.clone());
                    destructure.push((other.clone(), argument, ty));
                }
            }
        }
        let mut result_ty = Ty::Unit;
        let body = self.capture(|cx| {
            for (pat, argument, ty) in destructure {
                if let Ok((_, bindings)) = cx.pattern(&pat, &argument, &ty) {
                    for (name, value, ty) in bindings {
                        let variable = cx.bind(&name, ty);
                        cx.w.line(&format!("let {variable} = {value};"), None);
                    }
                }
            }
            if let Ok(value) = cx.expr(&mut closure.body, None) {
                result_ty = value.ty.clone();
                let js = cx.flush(value);
                cx.w.line(&format!("return {js};"), Some(span));
            }
        });
        self.pop_scope();
        Ok(V::new(
            format!("(({}) => {{\n{body}\n}})", names.join(", ")),
            Ty::Fn(Box::new(result_ty)),
        ))
    }

    // ------------------------------------------------ struct literals ----

    fn struct_literal(&mut self, expr: &mut syn::Expr, expect: Option<&Ty>) -> R<V> {
        let syn::Expr::Struct(literal) = expr else { return Err(()) };
        let span = literal.span();
        let segments: Vec<String> =
            literal.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
        let name = segments.last().cloned().unwrap_or_default();
        let owner =
            if segments.len() >= 2 { segments[segments.len() - 2].clone() } else { String::new() };
        // A module struct, or `Self`.
        let struct_name =
            if name == "Self" { self.self_ty.clone().unwrap_or_default() } else { name.clone() };
        if let Some(def) = self.module.structs.get(&struct_name).cloned() {
            let mut pre = Vec::new();
            let mut fields = Vec::new();
            for field in &mut literal.fields {
                let syn::Member::Named(member) = &field.member else {
                    return self.fail(
                        field.span(),
                        "a numbered field",
                        "write a tuple struct with its constructor",
                    );
                };
                let member = member.to_string();
                let Some((_, ty)) = def.fields.iter().find(|(name, _)| *name == member) else {
                    return self.fail(
                        field.span(),
                        &format!("`{member}`"),
                        &format!("`{struct_name}` has no such field"),
                    );
                };
                let value = self.expr(&mut field.expr, Some(ty))?;
                let value = self.copy_if_place(&field.expr, value);
                pre.extend(value.pre);
                fields.push(format!("{}: {}", string(&member), value.js));
            }
            let base = match &mut literal.rest {
                Some(rest) => {
                    let base = if is_default_call(rest) {
                        match self.module.default_js(&Ty::Adt(struct_name.clone()), span) {
                            Ok(js) => js,
                            Err(error) => {
                                self.error(error);
                                return Err(());
                            }
                        }
                    } else {
                        let value = self.expr(rest, Some(&Ty::Adt(struct_name.clone())))?;
                        pre.extend(value.pre);
                        format!("rn.clone({})", value.js)
                    };
                    format!("...{base}, ")
                }
                None => String::new(),
            };
            return Ok(seq(
                pre,
                format!("{{ {base}{} }}", fields.join(", ")),
                Ty::Adt(struct_name),
            ));
        }
        // A module enum's struct variant.
        if let Some(def) = self.module.enums.get(&owner).cloned() {
            let Some((_, Shape::Struct(shape))) =
                def.variants.iter().find(|(variant, _)| *variant == name)
            else {
                return self.fail(
                    span,
                    &format!("`{owner}::{name} {{ .. }}`"),
                    "no such struct variant",
                );
            };
            let mut pre = Vec::new();
            let mut fields = Vec::new();
            for field in &mut literal.fields {
                let syn::Member::Named(member) = &field.member else { return Err(()) };
                let member = member.to_string();
                let ty = shape.iter().find(|(name, _)| *name == member).map(|(_, ty)| ty.clone());
                let value = self.expr(&mut field.expr, ty.as_ref())?;
                pre.extend(value.pre);
                fields.push(format!("{}: {}", string(&member), value.js));
            }
            return Ok(seq(
                pre,
                format!("{{ {}: {{ {} }} }}", string(&name), fields.join(", ")),
                Ty::Adt(owner),
            ));
        }
        crate::methods::framework_struct(self, literal, &owner, &name, expect)
    }
}

/// A block's last statement written as a macro with no semicolon (`rsx! {}`,
/// `format!(..)`) is its value: syn reads it as a macro statement.
fn tail_macro(block: &mut syn::Block) {
    if let Some(syn::Stmt::Macro(mac)) = block.stmts.last() {
        if mac.semi_token.is_none() {
            let expr =
                syn::Expr::Macro(syn::ExprMacro { attrs: mac.attrs.clone(), mac: mac.mac.clone() });
            if let Some(last) = block.stmts.last_mut() {
                *last = syn::Stmt::Expr(expr, None);
            }
        }
    }
}

pub fn contains_let(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Let(_) => true,
        syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
            contains_let(&binary.left) || contains_let(&binary.right)
        }
        syn::Expr::Paren(inner) => contains_let(&inner.expr),
        _ => false,
    }
}

pub fn label_js(lifetime: &syn::Lifetime) -> String {
    format!("L_{}", lifetime.ident)
}

pub fn paren(js: &str) -> String {
    let simple = js.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.');
    if simple { js.to_owned() } else { format!("({js})") }
}

fn ident_field(name: &str) -> String {
    let _ = ident(name);
    name.to_owned()
}

/// `Default::default()` or `T::default()`.
pub fn is_default_call(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Call(call) if matches!(&*call.func, syn::Expr::Path(path) if last_segment(&path.path) == "default"))
}
