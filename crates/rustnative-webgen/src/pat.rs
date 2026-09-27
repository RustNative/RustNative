//! Patterns: a test on a value and the bindings it makes.
//!
//! Data crosses to the browser as serde's JSON, so a pattern tests that
//! shape: `None` is `null`, `Some(x)` is `x`, a unit variant is its name as
//! a string, and a variant with data is an object with one key — its name.
//! Framework events are objects tagged by `type`.

use syn::spanned::Spanned;

use crate::emit::string;
use crate::expr::R;
use crate::items::{Cx, Shape};
use crate::ty::{IntK, Ty, last_segment};

type Bindings = Vec<(String, String, Ty)>;

fn and(tests: Vec<String>) -> Option<String> {
    let tests: Vec<String> = tests.into_iter().filter(|test| test != "true").collect();
    if tests.is_empty() { None } else { Some(tests.join(" && ")) }
}

impl Cx<'_> {
    /// Tests `value` (a JavaScript expression of type `ty`) against `pat`.
    pub fn pattern(
        &mut self,
        pat: &syn::Pat,
        value: &str,
        ty: &Ty,
    ) -> R<(Option<String>, Bindings)> {
        let span = pat.span();
        match pat {
            syn::Pat::Wild(_) | syn::Pat::Rest(_) => Ok((None, Vec::new())),
            syn::Pat::Paren(inner) => self.pattern(&inner.pat, value, ty),
            syn::Pat::Reference(reference) => self.pattern(&reference.pat, value, ty),
            syn::Pat::Type(typed) => {
                let declared = self.module.read(&typed.ty);
                let ty = self.unify(&declared, ty);
                self.pattern(&typed.pat, value, &ty)
            }
            syn::Pat::Ident(binding) => {
                let name = binding.ident.to_string();
                if name == "None" && binding.subpat.is_none() {
                    return Ok((Some(format!("{value} === null")), Vec::new()));
                }
                // A unit variant brought in by `use`.
                if binding.subpat.is_none() {
                    if let Ty::Adt(adt) = ty {
                        if let Some(def) = self.module.enums.get(adt) {
                            if def.variants.iter().any(|(variant, shape)| {
                                *variant == name && matches!(shape, Shape::Unit)
                            }) {
                                return Ok((
                                    Some(format!("{value} === {}", string(&name))),
                                    Vec::new(),
                                ));
                            }
                        }
                    }
                }
                let mut bindings = vec![(name, value.to_owned(), ty.clone())];
                let mut test = None;
                if let Some((_, sub)) = &binding.subpat {
                    let (inner, more) = self.pattern(sub, value, ty)?;
                    test = inner;
                    bindings.extend(more);
                }
                Ok((test, bindings))
            }
            syn::Pat::Lit(literal) => {
                let mut expr = syn::Expr::Lit(literal.clone());
                let literal = self.expr(&mut expr, Some(ty))?;
                if let (Ty::Int(a), Ty::Int(b)) = (ty, &literal.ty) {
                    self.unify_int(*a, *b);
                }
                Ok((Some(format!("{value} === {}", literal.js)), Vec::new()))
            }
            syn::Pat::Range(range) => {
                let mut tests = Vec::new();
                if let Some(start) = &range.start {
                    let mut start = (**start).clone();
                    let start = self.expr(&mut start, Some(ty))?;
                    tests.push(if ty.is_numeric() {
                        format!("{value} >= {}", start.js)
                    } else {
                        format!("rn.cmp({value}, {}) >= 0", start.js)
                    });
                }
                if let Some(end) = &range.end {
                    let mut end = (**end).clone();
                    let end = self.expr(&mut end, Some(ty))?;
                    let closed = matches!(range.limits, syn::RangeLimits::Closed(_));
                    let op = if closed { "<=" } else { "<" };
                    tests.push(if ty.is_numeric() {
                        format!("{value} {op} {}", end.js)
                    } else {
                        format!("rn.cmp({value}, {}) {op} 0", end.js)
                    });
                }
                Ok((and(tests), Vec::new()))
            }
            syn::Pat::Or(or) => {
                let mut tests = Vec::new();
                let mut bindings: Option<Bindings> = None;
                for case in &or.cases {
                    let (test, case_bindings) = self.pattern(case, value, ty)?;
                    tests.push(format!("({})", test.unwrap_or_else(|| "true".into())));
                    if !case_bindings.is_empty() {
                        if bindings.is_some() {
                            return self.fail(
                                span,
                                "bindings in several alternatives",
                                "bind in one pattern per arm",
                            );
                        }
                        bindings = Some(case_bindings);
                    }
                }
                Ok((Some(tests.join(" || ")), bindings.unwrap_or_default()))
            }
            syn::Pat::Tuple(tuple) => {
                let items: Vec<Ty> = match ty {
                    Ty::Tuple(items) => items.clone(),
                    _ => vec![Ty::Opaque("_".into()); tuple.elems.len()],
                };
                let mut tests = Vec::new();
                let mut bindings = Vec::new();
                for (index, (element, item_ty)) in tuple.elems.iter().zip(items).enumerate() {
                    let (test, more) =
                        self.pattern(element, &format!("{value}[{index}]"), &item_ty)?;
                    tests.extend(test);
                    bindings.extend(more);
                }
                Ok((and(tests), bindings))
            }
            syn::Pat::TupleStruct(tuple) => self.tuple_struct(tuple, value, ty),
            syn::Pat::Path(path) => self.unit_path(&path.path, value, ty, span),
            syn::Pat::Struct(pattern) => self.struct_pattern(pattern, value, ty),
            syn::Pat::Slice(_) => {
                self.fail(span, "a slice pattern", "match on the length and index")
            }
            _ => self.fail(span, "this pattern", "the subset does not translate it"),
        }
    }

    fn unit_path(
        &mut self,
        path: &syn::Path,
        value: &str,
        ty: &Ty,
        span: proc_macro2::Span,
    ) -> R<(Option<String>, Bindings)> {
        let name = last_segment(path);
        if name == "None" {
            return Ok((Some(format!("{value} === null")), Vec::new()));
        }
        if path.segments.len() >= 2 {
            let owner = path.segments[path.segments.len() - 2].ident.to_string();
            let owner =
                if owner == "Self" { self.self_ty.clone().unwrap_or_default() } else { owner };
            if let Some(def) = self.module.enums.get(&owner) {
                if !def.variants.iter().any(|(variant, _)| *variant == name) {
                    return self.fail(span, &format!("`{owner}::{name}`"), "no such variant");
                }
            }
            // Module and framework unit variants alike are their names.
            return Ok((Some(format!("{value} === {}", string(&name))), Vec::new()));
        }
        let _ = ty;
        self.fail(span, "this path pattern", "name the variant with its enum")
    }

    fn tuple_struct(
        &mut self,
        tuple: &syn::PatTupleStruct,
        value: &str,
        ty: &Ty,
    ) -> R<(Option<String>, Bindings)> {
        let span = tuple.span();
        let name = last_segment(&tuple.path);
        let owner = if tuple.path.segments.len() >= 2 {
            tuple.path.segments[tuple.path.segments.len() - 2].ident.to_string()
        } else {
            String::new()
        };
        let owner = if owner == "Self" { self.self_ty.clone().unwrap_or_default() } else { owner };
        let elems: Vec<&syn::Pat> = tuple.elems.iter().collect();
        let single = |cx: &mut Self,
                      inner_value: String,
                      inner_ty: Ty,
                      test: String|
         -> R<(Option<String>, Bindings)> {
            let Some(first) = elems.first() else { return Ok((Some(test), Vec::new())) };
            let (inner, bindings) = cx.pattern(first, &inner_value, &inner_ty)?;
            Ok((and(std::iter::once(test).chain(inner).collect()), bindings))
        };
        match (owner.as_str(), name.as_str()) {
            ("", "Some") | ("Option", "Some") => {
                let inner = match ty {
                    Ty::Opt(inner) => (**inner).clone(),
                    _ => Ty::Opaque("_".into()),
                };
                single(self, value.to_owned(), inner, format!("{value} !== null"))
            }
            ("", "Ok") | ("Result", "Ok") | ("", "Err") | ("Result", "Err") => {
                let inner = match ty {
                    Ty::Res(ok, _) if name == "Ok" => (**ok).clone(),
                    Ty::Res(_, err) => (**err).clone(),
                    _ => Ty::Opaque("_".into()),
                };
                single(self, format!("{value}.{name}"), inner, format!("\"{name}\" in {value}"))
            }
            ("Event", "Lifecycle") => single(
                self,
                format!("{value}.value"),
                Ty::Lifecycle,
                format!("{value}.type === \"Lifecycle\""),
            ),
            ("KeyCode", "Character") => single(
                self,
                format!("{value}.Character"),
                Ty::Char,
                format!("rn.tag({value}) === \"Character\""),
            ),
            ("KeyCode", "Function") => single(
                self,
                format!("{value}.Function"),
                Ty::Int(IntK::U8),
                format!("rn.tag({value}) === \"Function\""),
            ),
            ("SocketEvent", "Message" | "Failed") => single(
                self,
                format!("{value}.{name}"),
                Ty::Str,
                format!("rn.tag({value}) === {}", string(&name)),
            ),
            ("SizeMode", "Fixed") => single(
                self,
                format!("{value}.Fixed"),
                Ty::Int(IntK::I32),
                format!("rn.tag({value}) === \"Fixed\""),
            ),
            _ => {
                if let Some(def) = self.module.enums.get(&owner).cloned() {
                    let Some((_, Shape::Tuple(fields))) =
                        def.variants.iter().find(|(variant, _)| *variant == name)
                    else {
                        return self.fail(
                            span,
                            &format!("`{owner}::{name}(..)`"),
                            "no such tuple variant",
                        );
                    };
                    let test = format!("rn.tag({value}) === {}", string(&name));
                    if fields.len() == 1 {
                        return single(self, format!("{value}.{name}"), fields[0].clone(), test);
                    }
                    let mut tests = vec![test];
                    let mut bindings = Vec::new();
                    for (index, (element, field_ty)) in elems.iter().zip(fields).enumerate() {
                        let (inner, more) =
                            self.pattern(element, &format!("{value}.{name}[{index}]"), field_ty)?;
                        tests.extend(inner);
                        bindings.extend(more);
                    }
                    return Ok((and(tests), bindings));
                }
                if let Some(def) = self.module.structs.get(&name).cloned() {
                    if def.fields.len() == 1 {
                        let field = def.fields[0].1.clone();
                        return single(self, value.to_owned(), field, "true".into());
                    }
                    let mut tests = Vec::new();
                    let mut bindings = Vec::new();
                    for (index, (element, (_, field_ty))) in
                        elems.iter().zip(def.fields).enumerate()
                    {
                        let (inner, more) =
                            self.pattern(element, &format!("{value}[{index}]"), &field_ty)?;
                        tests.extend(inner);
                        bindings.extend(more);
                    }
                    return Ok((and(tests), bindings));
                }
                self.fail(
                    span,
                    &format!("`{name}(..)`"),
                    "the subset matches module types, `Option`, `Result`, and framework events",
                )
            }
        }
    }

    fn struct_pattern(
        &mut self,
        pattern: &syn::PatStruct,
        value: &str,
        ty: &Ty,
    ) -> R<(Option<String>, Bindings)> {
        let span = pattern.span();
        let name = last_segment(&pattern.path);
        let owner = if pattern.path.segments.len() >= 2 {
            pattern.path.segments[pattern.path.segments.len() - 2].ident.to_string()
        } else {
            String::new()
        };
        let owner = if owner == "Self" { self.self_ty.clone().unwrap_or_default() } else { owner };
        let (test, base, fields): (Option<String>, String, Vec<(String, Ty)>) = if owner == "Event"
        {
            let Some(fields) = crate::methods::event_fields(&name) else {
                return self.fail(
                    span,
                    &format!("`Event::{name}`"),
                    "the browser does not deliver this event",
                );
            };
            (Some(format!("{value}.type === {}", string(&name))), value.to_owned(), fields)
        } else if let Some(fields) = crate::methods::framework_variant_fields(&owner, &name) {
            (
                Some(format!("rn.tag({value}) === {}", string(&name))),
                format!("{value}.{name}"),
                fields,
            )
        } else if let Some(def) = self.module.enums.get(&owner).cloned() {
            let Some((_, Shape::Struct(fields))) =
                def.variants.iter().find(|(variant, _)| *variant == name)
            else {
                return self.fail(
                    span,
                    &format!("`{owner}::{name} {{ .. }}`"),
                    "no such struct variant",
                );
            };
            (
                Some(format!("rn.tag({value}) === {}", string(&name))),
                format!("{value}.{name}"),
                fields.clone(),
            )
        } else if let Some(def) = self
            .module
            .structs
            .get(if name == "Self" {
                self.self_ty.as_deref().unwrap_or_default()
            } else {
                name.as_str()
            })
            .cloned()
        {
            (None, value.to_owned(), def.fields)
        } else if let Some(fields) = crate::methods::framework_struct_fields(&name) {
            (None, value.to_owned(), fields)
        } else {
            let _ = ty;
            return self.fail(
                span,
                &format!("`{name} {{ .. }}`"),
                "the subset matches module types and framework events",
            );
        };
        let mut tests: Vec<String> = test.into_iter().collect();
        let mut bindings = Vec::new();
        for field in &pattern.fields {
            let syn::Member::Named(member) = &field.member else {
                return self.fail(field.span(), "a numbered field", "name the field");
            };
            let member = member.to_string();
            let Some((_, field_ty)) = fields.iter().find(|(name, _)| *name == member) else {
                return self.fail(field.span(), &format!("`{member}`"), "no such field");
            };
            let (inner, more) = self.pattern(&field.pat, &format!("{base}.{member}"), field_ty)?;
            tests.extend(inner);
            bindings.extend(more);
        }
        Ok((and(tests), bindings))
    }
}
