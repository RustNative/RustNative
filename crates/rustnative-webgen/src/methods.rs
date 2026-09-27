//! Paths, calls, method calls, and macros: the standard library and
//! framework surface the subset translates, one table per receiver type.

use std::fmt::Write as _;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;

use crate::emit::{ident, string};
use crate::expr::{R, paren, seq};
use crate::items::{Cx, Shape, V};
use crate::ty::{IntK, Ty, last_segment};

const USIZE: Ty = Ty::Int(IntK::Usize);

/// The fields of a framework event the browser delivers.
pub fn event_fields(variant: &str) -> Option<Vec<(String, Ty)>> {
    let target = || ("target".to_owned(), Ty::NodeId);
    let optional_target = || ("target".to_owned(), Ty::Opt(Box::new(Ty::NodeId)));
    Some(match variant {
        "Click" | "FocusGained" | "FocusLost" | "PointerEnter" | "PointerLeave" => vec![target()],
        "TextChanged" => vec![target(), ("value".into(), Ty::Str)],
        "TextInput" => vec![optional_target(), ("text".into(), Ty::Str)],
        "KeyDown" | "KeyUp" => vec![
            optional_target(),
            ("key".into(), Ty::KeyCode),
            ("modifiers".into(), Ty::Modifiers),
        ],
        "TabSelected" => vec![target(), ("index".into(), USIZE)],
        "Toggled" => vec![target(), ("on".into(), Ty::Bool)],
        "ValueChanged" => vec![target(), ("value".into(), Ty::Int(IntK::I64))],
        "SelectionChanged" => vec![target(), ("index".into(), Ty::Opt(Box::new(USIZE)))],
        "DateChanged" => vec![target(), ("date".into(), Ty::Date)],
        "DeepLink" => vec![("url".into(), Ty::Str)],
        "PointerDown" | "PointerMove" | "PointerUp" | "PointerCancel" => {
            vec![target(), ("pointer".into(), Ty::Opaque("PointerEvent".into()))]
        }
        "Wheel" => vec![target(), ("delta".into(), Ty::Opaque("WheelDelta".into()))],
        "Composition" => {
            vec![optional_target(), ("composition".into(), Ty::Opaque("Composition".into()))]
        }
        "Clipboard" => {
            vec![optional_target(), ("action".into(), Ty::Opaque("ClipboardAction".into()))]
        }
        "VisibleRangeChanged" => {
            vec![target(), ("range".into(), Ty::Opaque("VirtualRange".into()))]
        }
        _ => return None,
    })
}

/// A framework struct's fields, for patterns.
pub fn framework_struct_fields(name: &str) -> Option<Vec<(String, Ty)>> {
    Some(match name {
        "KeyModifiers" => ["shift", "ctrl", "alt", "meta"]
            .iter()
            .map(|field| ((*field).to_owned(), Ty::Bool))
            .collect(),
        "CalendarDate" => vec![
            ("year".into(), Ty::Int(IntK::I32)),
            ("month".into(), Ty::Int(IntK::U8)),
            ("day".into(), Ty::Int(IntK::U8)),
        ],
        "EdgeInsets" => ["top", "end", "bottom", "start"]
            .iter()
            .map(|field| ((*field).to_owned(), Ty::Int(IntK::I32)))
            .collect(),
        "Typography" => vec![
            ("family".into(), Ty::Str),
            ("size".into(), Ty::Int(IntK::U16)),
            ("weight".into(), Ty::Int(IntK::U16)),
        ],
        _ => return None,
    })
}

/// A field of a framework value.
pub fn framework_field(ty: &Ty, name: &str) -> Option<Ty> {
    let fields = match ty {
        Ty::Modifiers => framework_struct_fields("KeyModifiers")?,
        Ty::Date => framework_struct_fields("CalendarDate")?,
        Ty::Insets => framework_struct_fields("EdgeInsets")?,
        Ty::Typography => framework_struct_fields("Typography")?,
        Ty::Opaque(kind) if kind == "VirtualRange" => {
            vec![("first".into(), USIZE), ("last_exclusive".into(), USIZE)]
        }
        // Data from outside the module: its fields are read by name, and
        // their types are unknown.
        Ty::Opaque(_) => return Some(Ty::Opaque(format!(".{name}"))),
        _ => return None,
    };
    fields.into_iter().find(|(field, _)| field == name).map(|(_, ty)| ty)
}

/// A framework enum's unit variant: its JavaScript and type.
fn unit_variant(owner: &str, name: &str) -> Option<(String, Ty)> {
    let quoted = || string(name);
    Some(match owner {
        "SizeMode" if matches!(name, "Auto" | "Fill") => (quoted(), Ty::SizeMode),
        "Alignment" => (quoted(), Ty::Alignment),
        "Overflow" => (quoted(), Ty::Overflow),
        "LayoutDirection" => (quoted(), Ty::Direction),
        "Track" if name == "Auto" => (quoted(), Ty::Track),
        "Axis" => (quoted(), Ty::Axis),
        "Cursor" => (quoted(), Ty::Cursor),
        "ControlState" => (
            string(match name {
                "Hovered" => "hover",
                "Focused" => "focus",
                "Pressed" => "active",
                "Disabled" => "disabled",
                _ => "normal",
            }),
            Ty::ControlState,
        ),
        "CheckedState" => (
            string(match name {
                "Checked" => "true",
                "Unchecked" => "false",
                _ => "mixed",
            }),
            Ty::Checked,
        ),
        "LiveRegion" => (
            match name {
                "Polite" => string("polite"),
                "Assertive" => string("assertive"),
                _ => "null".into(),
            },
            Ty::Live,
        ),
        "KeyCode" => (quoted(), Ty::KeyCode),
        "Lifecycle" => (quoted(), Ty::Lifecycle),
        "AccessibilityRole" => (quoted(), Ty::Role),
        "Control" if name == "Separator" => (quoted(), Ty::Control),
        _ => return None,
    })
}

fn int_const(kind: &str, name: &str) -> Option<String> {
    let (min, max): (i64, i64) = match kind {
        "i8" => (-128, 127),
        "i16" => (-32_768, 32_767),
        "i32" => (-2_147_483_648, 2_147_483_647),
        "u8" => (0, 255),
        "u16" => (0, 65_535),
        "u32" => (0, 4_294_967_295),
        _ => return None,
    };
    match name {
        "MAX" => Some(max.to_string()),
        "MIN" => Some(min.to_string()),
        _ => None,
    }
}

fn float_const(name: &str) -> Option<&'static str> {
    Some(match name {
        "PI" => "Math.PI",
        "E" => "Math.E",
        "TAU" => "(2 * Math.PI)",
        "SQRT_2" => "Math.SQRT2",
        "LN_2" => "Math.LN2",
        "LN_10" => "Math.LN10",
        "INFINITY" => "Infinity",
        "NEG_INFINITY" => "(-Infinity)",
        "NAN" => "NaN",
        "EPSILON" => "Number.EPSILON",
        "MAX" => "Number.MAX_VALUE",
        "MIN" => "(-Number.MAX_VALUE)",
        "MIN_POSITIVE" => "2.2250738585072014e-308",
        _ => return None,
    })
}

/// Arguments of a call, each translated against its expected type.
fn arguments(
    cx: &mut Cx<'_>,
    args: &mut syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    expected: &[Ty],
) -> R<(Vec<String>, Vec<String>, Vec<Ty>)> {
    let mut pre = Vec::new();
    let mut js = Vec::new();
    let mut types = Vec::new();
    for (index, arg) in args.iter_mut().enumerate() {
        let expect = expected.get(index);
        let value = if let (syn::Expr::Closure(closure), Some(Ty::Fn(_) | Ty::Tuple(_))) =
            (&mut *arg, expect)
        {
            cx.closure(closure, expect)?
        } else {
            cx.expr(arg, expect)?
        };
        let value = cx.copy_if_place(arg, value);
        if let (Some(expect), false) = (expect, matches!(value.ty, Ty::Fn(_))) {
            cx.unify(expect, &value.ty);
        }
        pre.extend(value.pre);
        js.push(value.js);
        types.push(value.ty);
    }
    Ok((pre, js, types))
}

fn variant_constructor(name: &str, fields: usize) -> String {
    match fields {
        0 => string(name),
        1 => format!("((a) => ({{ {}: a }}))", string(name)),
        n => {
            let params: Vec<String> = (0..n).map(|index| format!("a{index}")).collect();
            format!(
                "(({}) => ({{ {}: [{}] }}))",
                params.join(", "),
                string(name),
                params.join(", ")
            )
        }
    }
}

/// A closure argument: its parameter types.
fn closure_arg(cx: &mut Cx<'_>, arg: &mut syn::Expr, params: &[Ty]) -> R<V> {
    let span = arg.span();
    match arg {
        syn::Expr::Closure(closure) => cx.closure_with(closure, params, span),
        other => cx.expr(other, Some(&Ty::Fn(Box::new(Ty::Opaque("_".into()))))),
    }
}

fn result_of(ty: &Ty) -> Ty {
    match ty {
        Ty::Fn(output) => (**output).clone(),
        other => other.clone(),
    }
}

impl Cx<'_> {
    // ------------------------------------------------------------ paths ----

    pub fn path_value(&mut self, path: &mut syn::ExprPath, expect: Option<&Ty>) -> R<V> {
        let span = path.span();
        if path.qself.is_some() {
            return self.fail(span, "a qualified path", "call through the type");
        }
        let segments: Vec<String> =
            path.path.segments.iter().map(|segment| segment.ident.to_string()).collect();
        if segments.len() == 1 {
            let name = &segments[0];
            if let Some(binding) = self.lookup(name) {
                return Ok(V::new(binding.js.clone(), binding.ty.clone()));
            }
            if name == "None" {
                let ty = match expect {
                    Some(Ty::Opt(_)) => expect.cloned().unwrap_or(Ty::Unit),
                    _ => Ty::Opt(Box::new(Ty::Opaque("_".into()))),
                };
                return Ok(V::new("null", ty));
            }
            if let Some(ty) = self.module.consts.get(name) {
                return Ok(V::new(ident(name), ty.clone()));
            }
            if let Some(sig) = self.module.fns.get(name) {
                return Ok(V::new(ident(name), Ty::Fn(Box::new(sig.ret.clone()))));
            }
            match name.as_str() {
                "Some" => {
                    return Ok(V::new(
                        "((x) => x)",
                        Ty::Fn(Box::new(Ty::Opt(Box::new(Ty::Opaque("_".into()))))),
                    ));
                }
                "Ok" | "Err" => {
                    return Ok(V::new(
                        variant_constructor(name, 1),
                        Ty::Fn(Box::new(Ty::Opaque("Result".into()))),
                    ));
                }
                _ => {}
            }
            return self.fail(
                span,
                &format!("`{name}`"),
                "it is not a variable, constant, or function of the client module",
            );
        }
        let owner = segments[segments.len() - 2].clone();
        let owner = if owner == "Self" { self.self_ty.clone().unwrap_or_default() } else { owner };
        let name = segments.last().cloned().unwrap_or_default();
        if let Some(def) = self.module.enums.get(&owner).cloned() {
            let Some((_, shape)) = def.variants.iter().find(|(variant, _)| *variant == name) else {
                return self.fail(span, &format!("`{owner}::{name}`"), "no such variant");
            };
            return Ok(match shape {
                Shape::Unit => V::new(string(&name), Ty::Adt(owner)),
                Shape::Tuple(fields) => V::new(
                    variant_constructor(&name, fields.len()),
                    Ty::Fn(Box::new(Ty::Adt(owner))),
                ),
                Shape::Struct(_) => {
                    return self.fail(
                        span,
                        &format!("`{owner}::{name}`"),
                        "a struct variant is built with its fields",
                    );
                }
            });
        }
        if let Some(sig) = self.module.methods.get(&(owner.clone(), name.clone())) {
            return Ok(V::new(format!("{owner}${name}"), Ty::Fn(Box::new(sig.ret.clone()))));
        }
        if let Some((js, ty)) = unit_variant(&owner, &name) {
            return Ok(V::new(js, ty));
        }
        if let Some(value) = int_const(&owner, &name) {
            return Ok(V::new(value, Ty::Int(IntK::from_name(&owner).unwrap_or(IntK::I32))));
        }
        if matches!(owner.as_str(), "i64" | "u64" | "isize" | "usize")
            && matches!(name.as_str(), "MAX" | "MIN")
        {
            return self.fail(
                span,
                &format!("`{owner}::{name}`"),
                "it is beyond ±(2^53 − 1), which JavaScript cannot represent exactly",
            );
        }
        if (owner == "f64" || owner == "f32" || owner == "consts") && float_const(&name).is_some() {
            let f32 = segments.iter().any(|segment| segment == "f32");
            return Ok(V::new(float_const(&name).unwrap_or("NaN"), Ty::Float(f32)));
        }
        if owner == "Option" && name == "Some" {
            return Ok(V::new(
                "((x) => x)",
                Ty::Fn(Box::new(Ty::Opt(Box::new(Ty::Opaque("_".into()))))),
            ));
        }
        self.fail(span, &format!("`{}`", segments.join("::")), "the subset does not know it")
    }

    // ------------------------------------------------------------ calls ----

    pub fn call(&mut self, expr: &mut syn::Expr, expect: Option<&Ty>) -> R<V> {
        let syn::Expr::Call(call) = expr else { return Err(()) };
        let span = call.span();
        let (segments, qself_ty) = match &*call.func {
            syn::Expr::Path(path) => (
                path.path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>(),
                path.qself.as_ref().map(|qself| self.module.read(&qself.ty)),
            ),
            _ => {
                let callee = self.expr(&mut call.func, None)?;
                let (pre, args, _) = arguments(self, &mut call.args, &[])?;
                let mut all = callee.pre;
                all.extend(pre);
                return Ok(seq(
                    all,
                    format!("{}({})", paren(&callee.js), args.join(", ")),
                    result_of(&callee.ty),
                ));
            }
        };
        let name = segments.last().cloned().unwrap_or_default();
        // `<T as Default>::default()`.
        if let Some(ty) = qself_ty {
            if name == "default" {
                return self.default_value(&ty, span);
            }
            return self.fail(span, "a qualified call", "call the method directly");
        }
        if segments.len() == 1 {
            if let Some(binding) = self.lookup(&name).cloned() {
                let (pre, args, _) = arguments(self, &mut call.args, &[])?;
                return Ok(seq(
                    pre,
                    format!("{}({})", binding.js, args.join(", ")),
                    result_of(&binding.ty),
                ));
            }
            match name.as_str() {
                "Some" => {
                    let inner = match expect {
                        Some(Ty::Opt(inner)) => Some((**inner).clone()),
                        _ => None,
                    };
                    let (pre, args, types) =
                        arguments(self, &mut call.args, &inner.into_iter().collect::<Vec<_>>())?;
                    return Ok(seq(
                        pre,
                        args.join(""),
                        Ty::Opt(Box::new(types.into_iter().next().unwrap_or(Ty::Unit))),
                    ));
                }
                "Ok" | "Err" => {
                    let (ok, err) = match expect {
                        Some(Ty::Res(ok, err)) => ((**ok).clone(), (**err).clone()),
                        _ => (Ty::Opaque("_".into()), Ty::Opaque("_".into())),
                    };
                    let expected = if name == "Ok" { vec![ok.clone()] } else { vec![err.clone()] };
                    let (pre, args, types) = arguments(self, &mut call.args, &expected)?;
                    let value = types.into_iter().next().unwrap_or(Ty::Unit);
                    let ty = if name == "Ok" {
                        Ty::Res(Box::new(value), Box::new(err))
                    } else {
                        Ty::Res(Box::new(ok), Box::new(value))
                    };
                    return Ok(seq(pre, format!("{{ {name}: {} }}", args.join("")), ty));
                }
                _ => {}
            }
            if let Some(sig) = self.module.fns.get(&name).cloned() {
                let (pre, args, _) = arguments(self, &mut call.args, &sig.params)?;
                return Ok(seq(pre, format!("{}({})", ident(&name), args.join(", ")), sig.ret));
            }
            if let Some(def) = self.module.structs.get(&name).cloned() {
                if def.tuple {
                    let types: Vec<Ty> = def.fields.iter().map(|(_, ty)| ty.clone()).collect();
                    let (pre, args, _) = arguments(self, &mut call.args, &types)?;
                    let js = if args.len() == 1 {
                        args[0].clone()
                    } else {
                        format!("[{}]", args.join(", "))
                    };
                    return Ok(seq(pre, js, Ty::Adt(name)));
                }
            }
            return self.fail(
                span,
                &format!("`{name}(..)`"),
                "it is not a function of the client module",
            );
        }
        let owner = segments[segments.len() - 2].clone();
        let owner = if owner == "Self" { self.self_ty.clone().unwrap_or_default() } else { owner };
        // A module enum's tuple variant.
        if let Some(def) = self.module.enums.get(&owner).cloned() {
            if let Some((_, Shape::Tuple(fields))) =
                def.variants.iter().find(|(variant, _)| *variant == name)
            {
                let (pre, args, _) = arguments(self, &mut call.args, fields)?;
                let js = if args.len() == 1 {
                    format!("{{ {}: {} }}", string(&name), args[0])
                } else {
                    format!("{{ {}: [{}] }}", string(&name), args.join(", "))
                };
                return Ok(seq(pre, js, Ty::Adt(owner)));
            }
        }
        // A module type's associated function or method.
        if let Some(sig) = self.module.methods.get(&(owner.clone(), name.clone())).cloned() {
            let mut expected = Vec::new();
            if sig.method {
                expected.push(Ty::Adt(owner.clone()));
            }
            expected.extend(sig.params.iter().cloned());
            let (pre, args, _) = arguments(self, &mut call.args, &expected)?;
            return Ok(seq(pre, format!("{owner}${name}({})", args.join(", ")), sig.ret));
        }
        if name == "default" && (self.module.is_local(&owner) || owner == "Default") {
            let ty = if owner == "Default" {
                expect.cloned().unwrap_or(Ty::Unit)
            } else {
                Ty::Adt(owner)
            };
            return self.default_value(&ty, span);
        }
        self.framework_call(&owner, &name, call, expect, span)
    }

    fn default_value(&mut self, ty: &Ty, span: Span) -> R<V> {
        match self.module.default_js(ty, span) {
            Ok(js) => Ok(V::new(js, ty.clone())),
            Err(error) => {
                self.error(error);
                Err(())
            }
        }
    }

    #[allow(clippy::cognitive_complexity, reason = "one arm per framework function")]
    fn framework_call(
        &mut self,
        owner: &str,
        name: &str,
        call: &mut syn::ExprCall,
        expect: Option<&Ty>,
        span: Span,
    ) -> R<V> {
        let int = |kind: IntK| Ty::Int(kind);
        let layout = Ty::Layout;
        let simple = |cx: &mut Self,
                      call: &mut syn::ExprCall,
                      expected: &[Ty],
                      js: &dyn Fn(&[String]) -> String,
                      ty: Ty|
         -> R<V> {
            let (pre, args, _) = arguments(cx, &mut call.args, expected)?;
            Ok(seq(pre, js(&args), ty))
        };
        let node_key = |args: &[String]| format!("String({})", args[0]);
        match (owner, name) {
            ("Node", "label") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str],
                &|a| format!("rn.label({}, {})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "label_with_layout") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str, layout.clone()],
                &|a| format!("rn.label({}, {}, {})", node_key(a), a[1], a[2]),
                Ty::Node,
            ),
            ("Node", "button") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str],
                &|a| format!("rn.button({}, {})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "button_with_layout") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str, layout.clone()],
                &|a| format!("rn.button({}, {}, {})", node_key(a), a[1], a[2]),
                Ty::Node,
            ),
            ("Node", "text_input") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str],
                &|a| format!("rn.textInput({}, {})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "text_input_with_layout") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str, layout.clone()],
                &|a| format!("rn.textInput({}, {}, {})", node_key(a), a[1], a[2]),
                Ty::Node,
            ),
            ("Node", "column") => simple(
                self,
                call,
                &[Ty::Str, Ty::Vec(Box::new(Ty::Node))],
                &|a| format!("rn.column({}, {})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "column_with_layout") => simple(
                self,
                call,
                &[Ty::Str, Ty::Vec(Box::new(Ty::Node)), layout.clone(), Ty::Container],
                &|a| format!("rn.column({}, {}, {}, {})", node_key(a), a[1], a[2], a[3]),
                Ty::Node,
            ),
            ("Node", "row") => simple(
                self,
                call,
                &[Ty::Str, Ty::Vec(Box::new(Ty::Node))],
                &|a| format!("rn.row({}, {})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "row_with_layout") => simple(
                self,
                call,
                &[Ty::Str, Ty::Vec(Box::new(Ty::Node)), layout.clone(), Ty::Container],
                &|a| format!("rn.row({}, {}, {}, {})", node_key(a), a[1], a[2], a[3]),
                Ty::Node,
            ),
            ("Node", "grid") => simple(
                self,
                call,
                &[Ty::Str, Ty::Grid, layout.clone(), Ty::Vec(Box::new(Ty::Node))],
                &|a| format!("rn.grid({}, {}, {}, {})", node_key(a), a[1], a[2], a[3]),
                Ty::Node,
            ),
            ("Node", "virtual_list_with_layout") => simple(
                self,
                call,
                &[Ty::Str, Ty::VirtualList, layout.clone(), Ty::Vec(Box::new(Ty::Node))],
                &|a| format!("rn.virtualList({}, {}, {}, {})", node_key(a), a[1], a[2], a[3]),
                Ty::Node,
            ),
            ("Node", "virtual_list") => simple(
                self,
                call,
                &[Ty::Str, Ty::VirtualList, Ty::Vec(Box::new(Ty::Node))],
                &|a| format!("rn.virtualList({}, {}, rn.layout(), {})", node_key(a), a[1], a[2]),
                Ty::Node,
            ),
            ("Node", "tab_bar") => simple(
                self,
                call,
                &[Ty::Str, Ty::Vec(Box::new(Ty::Str)), USIZE, layout.clone()],
                &|a| format!("rn.tabBar({}, [...{}], {}, {})", node_key(a), a[1], a[2], a[3]),
                Ty::Node,
            ),
            ("Node", "control") => simple(
                self,
                call,
                &[Ty::Str, Ty::Control],
                &|a| format!("rn.control({}, {})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "control_with_layout") => simple(
                self,
                call,
                &[Ty::Str, Ty::Control, layout.clone()],
                &|a| format!("rn.control({}, {}, {})", node_key(a), a[1], a[2]),
                Ty::Node,
            ),
            ("Node", "checkbox") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str, Ty::Bool],
                &|a| {
                    format!(
                        "rn.control({}, {{ Checkbox: {{ label: {}, checked: {} }} }})",
                        node_key(a),
                        a[1],
                        a[2]
                    )
                },
                Ty::Node,
            ),
            ("Node", "toggle") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str, Ty::Bool],
                &|a| {
                    format!(
                        "rn.control({}, {{ Toggle: {{ label: {}, on: {} }} }})",
                        node_key(a),
                        a[1],
                        a[2]
                    )
                },
                Ty::Node,
            ),
            ("Node", "radio") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str, Ty::Bool],
                &|a| {
                    format!(
                        "rn.control({}, {{ Radio: {{ label: {}, selected: {} }} }})",
                        node_key(a),
                        a[1],
                        a[2]
                    )
                },
                Ty::Node,
            ),
            ("Node", "slider") => simple(
                self,
                call,
                &[Ty::Str, int(IntK::I64), int(IntK::I64), int(IntK::I64)],
                &|a| {
                    format!(
                        "rn.control({}, {{ Slider: {{ value: {}, min: {}, max: {} }} }})",
                        node_key(a),
                        a[1],
                        a[2],
                        a[3]
                    )
                },
                Ty::Node,
            ),
            ("Node", "spinner") => simple(
                self,
                call,
                &[Ty::Str, int(IntK::I64), int(IntK::I64), int(IntK::I64)],
                &|a| {
                    format!(
                        "rn.control({}, {{ Spinner: {{ value: {}, min: {}, max: {} }} }})",
                        node_key(a),
                        a[1],
                        a[2],
                        a[3]
                    )
                },
                Ty::Node,
            ),
            ("Node", "progress") => simple(
                self,
                call,
                &[Ty::Str, Ty::Opt(Box::new(int(IntK::U8)))],
                &|a| {
                    format!("rn.control({}, {{ Progress: {{ percent: {} }} }})", node_key(a), a[1])
                },
                Ty::Node,
            ),
            ("Node", "select") => simple(
                self,
                call,
                &[Ty::Str, Ty::Vec(Box::new(Ty::Str)), Ty::Opt(Box::new(USIZE))],
                &|a| {
                    format!(
                        "rn.control({}, {{ Select: {{ options: [...{}].map(String), selected: {} }} }})",
                        node_key(a),
                        a[1],
                        a[2]
                    )
                },
                Ty::Node,
            ),
            ("Node", "list_box") => simple(
                self,
                call,
                &[Ty::Str, Ty::Vec(Box::new(Ty::Str)), Ty::Opt(Box::new(USIZE))],
                &|a| {
                    format!(
                        "rn.control({}, {{ ListBox: {{ items: [...{}].map(String), selected: {} }} }})",
                        node_key(a),
                        a[1],
                        a[2]
                    )
                },
                Ty::Node,
            ),
            ("Node", "date_picker") => simple(
                self,
                call,
                &[Ty::Str, Ty::Date],
                &|a| format!("rn.control({}, {{ DatePicker: {{ date: {} }} }})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "separator") => simple(
                self,
                call,
                &[Ty::Str],
                &|a| format!("rn.control({}, \"Separator\")", node_key(a)),
                Ty::Node,
            ),
            ("Node", "link") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str],
                &|a| format!("rn.control({}, {{ Link: {{ text: {} }} }})", node_key(a), a[1]),
                Ty::Node,
            ),
            ("Node", "multiline_text") => simple(
                self,
                call,
                &[Ty::Str, Ty::Str],
                &|a| {
                    format!(
                        "rn.control({}, {{ MultilineText: {{ value: {} }} }})",
                        node_key(a),
                        a[1]
                    )
                },
                Ty::Node,
            ),
            ("Node", other) => self.fail(
                span,
                &format!("`Node::{other}`"),
                "drawn, native, and foreign content is not in the subset",
            ),
            ("String", "new") => Ok(V::new("\"\"", Ty::Str)),
            ("String", "from" | "from_str") => {
                simple(self, call, &[Ty::Str], &|a| format!("String({})", a[0]), Ty::Str)
            }
            ("String", "with_capacity") => {
                simple(self, call, &[USIZE], &|_| "\"\"".into(), Ty::Str)
            }
            ("Vec", "new") => Ok(V::new(
                "[]",
                expect
                    .filter(|ty| matches!(ty, Ty::Vec(_)))
                    .cloned()
                    .unwrap_or_else(|| Ty::Vec(Box::new(Ty::Opaque("_".into())))),
            )),
            ("Vec", "with_capacity") => {
                let _ = arguments(self, &mut call.args, &[USIZE])?;
                Ok(V::new(
                    "[]",
                    expect
                        .filter(|ty| matches!(ty, Ty::Vec(_)))
                        .cloned()
                        .unwrap_or_else(|| Ty::Vec(Box::new(Ty::Opaque("_".into())))),
                ))
            }
            ("NodeId", "from_key") => {
                simple(self, call, &[Ty::Str], &|a| format!("String({})", a[0]), Ty::NodeId)
            }
            ("LayoutStyle", "new" | "default") => Ok(V::new("rn.layout()", Ty::Layout)),
            ("ColumnStyle" | "RowStyle", "new" | "default") => {
                Ok(V::new("rn.containerStyle()", Ty::Container))
            }
            ("EdgeInsets", "all") => simple(
                self,
                call,
                &[int(IntK::I32)],
                &|a| format!("rn.insets({0}, {0}, {0}, {0})", a[0]),
                Ty::Insets,
            ),
            ("EdgeInsets", "symmetric") => simple(
                self,
                call,
                &[int(IntK::I32), int(IntK::I32)],
                &|a| format!("rn.insets({0}, {1}, {0}, {1})", a[0], a[1]),
                Ty::Insets,
            ),
            ("EdgeInsets", "logical") => simple(
                self,
                call,
                &[int(IntK::I32), int(IntK::I32), int(IntK::I32), int(IntK::I32)],
                &|a| format!("rn.insets({}, {}, {}, {})", a[0], a[1], a[2], a[3]),
                Ty::Insets,
            ),
            ("Constraints", "new" | "default") => Ok(V::new(
                "{ min_width: 0, max_width: null, min_height: 0, max_height: null }",
                Ty::Constraints,
            )),
            ("GridStyle", "new") => simple(
                self,
                call,
                &[Ty::Vec(Box::new(Ty::Track))],
                &|a| {
                    format!(
                        "{{ columns: [...{}], rows: [], gap: 0, padding: rn.insets(0, 0, 0, 0) }}",
                        a[0]
                    )
                },
                Ty::Grid,
            ),
            ("GridPlacement", "at") => simple(
                self,
                call,
                &[USIZE, USIZE],
                &|a| format!("{{ row: {}, column: {}, row_span: 1, column_span: 1 }}", a[0], a[1]),
                Ty::Placement,
            ),
            ("Track", "Fixed") => simple(
                self,
                call,
                &[int(IntK::I32)],
                &|a| format!("{{ Fixed: {} }}", a[0]),
                Ty::Track,
            ),
            ("Track", "Fraction") => simple(
                self,
                call,
                &[int(IntK::U16)],
                &|a| format!("{{ Fraction: {} }}", a[0]),
                Ty::Track,
            ),
            ("SizeMode", "Fixed") => simple(
                self,
                call,
                &[int(IntK::I32)],
                &|a| format!("{{ Fixed: {} }}", a[0]),
                Ty::SizeMode,
            ),
            ("ItemExtent", "Fixed" | "Estimated") => simple(
                self,
                call,
                &[int(IntK::U32)],
                &|a| format!("{{ {name}: {} }}", a[0]),
                Ty::Extent,
            ),
            ("VirtualListStyle", "new") => simple(
                self,
                call,
                &[USIZE, Ty::Extent],
                &|a| {
                    format!(
                        "{{ item_count: {}, extent: {}, overscan: 2, axis: \"Vertical\" }}",
                        a[0], a[1]
                    )
                },
                Ty::VirtualList,
            ),
            ("AccessibilityInfo", "new") => {
                simple(self, call, &[Ty::Role], &|a| format!("rn.a11yOf({})", a[0]), Ty::A11y)
            }
            ("VisualStyle", "new" | "default") => Ok(V::new("rn.visual()", Ty::Visual)),
            ("Color", "rgb") => simple(
                self,
                call,
                &[int(IntK::U8), int(IntK::U8), int(IntK::U8)],
                &|a| format!("rn.hex({}, {}, {})", a[0], a[1], a[2]),
                Ty::Color,
            ),
            ("Color", "rgba") => simple(
                self,
                call,
                &[int(IntK::U8), int(IntK::U8), int(IntK::U8), int(IntK::U8)],
                &|a| format!("rn.hex({}, {}, {}, {})", a[0], a[1], a[2], a[3]),
                Ty::Color,
            ),
            ("CalendarDate", "new") => simple(
                self,
                call,
                &[int(IntK::I32), int(IntK::U8), int(IntK::U8)],
                &|a| format!("rn.date({}, {}, {})", a[0], a[1], a[2]),
                Ty::Opt(Box::new(Ty::Date)),
            ),
            ("KeyCode", "Character") => simple(
                self,
                call,
                &[Ty::Char],
                &|a| format!("{{ Character: {} }}", a[0]),
                Ty::KeyCode,
            ),
            ("KeyCode", "Function") => simple(
                self,
                call,
                &[int(IntK::U8)],
                &|a| format!("{{ Function: {} }}", a[0]),
                Ty::KeyCode,
            ),
            ("Box" | "Rc" | "Arc", "new") => {
                let (pre, args, types) = arguments(
                    self,
                    &mut call.args,
                    &expect.into_iter().cloned().collect::<Vec<_>>(),
                )?;
                Ok(seq(pre, args.join(""), types.into_iter().next().unwrap_or(Ty::Unit)))
            }
            ("mem", "take") => {
                let Some(argument) = call.args.first_mut() else { return Err(()) };
                let (mut pre, place, ty) = self.place(strip_ref(argument))?;
                let default = match self.module.default_js(&ty, span) {
                    Ok(js) => js,
                    Err(error) => {
                        self.error(error);
                        return Err(());
                    }
                };
                let taken = self.temp();
                pre.push(format!("const {taken} = {place};"));
                pre.push(format!("{place} = {default};"));
                Ok(seq(pre, taken, ty))
            }
            ("mem", "replace") => {
                let mut args = call.args.iter_mut();
                let (Some(target), Some(value)) = (args.next(), args.next()) else {
                    return Err(());
                };
                let (mut pre, place, ty) = self.place(strip_ref(target))?;
                let value = self.expr(value, Some(&ty))?;
                pre.extend(value.pre);
                let taken = self.temp();
                pre.push(format!("const {taken} = {place};"));
                pre.push(format!("{place} = {};", value.js));
                Ok(seq(pre, taken, ty))
            }
            ("mem", "swap") => {
                let mut args = call.args.iter_mut();
                let (Some(a), Some(b)) = (args.next(), args.next()) else { return Err(()) };
                let (mut pre, a, ty) = self.place(strip_ref(a))?;
                let (more, b, _) = self.place(strip_ref(b))?;
                pre.extend(more);
                pre.push(format!("[{a}, {b}] = [{b}, {a}];"));
                Ok(seq(pre, "undefined", ty))
            }
            ("cmp", "max" | "min") => {
                let (pre, args, types) = arguments(self, &mut call.args, &[])?;
                let ty = self.unify(&types[0], &types[1]);
                let js = if ty.is_numeric() {
                    format!("Math.{name}({}, {})", args[0], args[1])
                } else if name == "max" {
                    format!("(rn.cmp({0}, {1}) > 0 ? {0} : {1})", args[0], args[1])
                } else {
                    format!("(rn.cmp({0}, {1}) <= 0 ? {0} : {1})", args[0], args[1])
                };
                Ok(seq(pre, js, ty))
            }
            (kind, "from") if IntK::from_name(kind).is_some() || kind == "f64" || kind == "f32" => {
                let (pre, args, _) = arguments(self, &mut call.args, &[])?;
                let ty = crate::ty::read(&syn::parse_str(kind).map_err(|_| ())?, &|_| false);
                let js = match (&ty, args[0].as_str()) {
                    (Ty::Float(true), value) => format!("Math.fround({value})"),
                    (_, value) => format!(
                        "rn.castInt(Number({value}), {})",
                        self.kind_js(match ty {
                            Ty::Int(kind) => kind,
                            _ => IntK::I32,
                        })
                    ),
                };
                let js = if matches!(ty, Ty::Float(false)) {
                    format!("Number({})", args[0])
                } else {
                    js
                };
                Ok(seq(pre, js, ty))
            }
            (kind, "try_from") if IntK::from_name(kind).is_some() => {
                let kind = IntK::from_name(kind).unwrap_or(IntK::I32);
                let (pre, args, _) = arguments(self, &mut call.args, &[])?;
                Ok(seq(
                    pre,
                    format!("rn.tryFrom({}, {})", args[0], self.kind_js(kind)),
                    Ty::Res(Box::new(Ty::Int(kind)), Box::new(Ty::ParseError)),
                ))
            }
            ("char", "from") => simple(
                self,
                call,
                &[int(IntK::U8)],
                &|a| format!("String.fromCodePoint({})", a[0]),
                Ty::Char,
            ),
            ("char", "from_u32") => simple(
                self,
                call,
                &[int(IntK::U32)],
                &|a| format!("rn.charFromU32({})", a[0]),
                Ty::Opt(Box::new(Ty::Char)),
            ),
            ("IntoIterator", "into_iter") => {
                let (pre, args, types) = arguments(self, &mut call.args, &[])?;
                let element = types[0].element().unwrap_or(Ty::Opaque("_".into()));
                Ok(seq(pre, format!("[...{}]", paren(&args[0])), Ty::Iter(Box::new(element))))
            }
            ("Into" | "From", "into" | "from") => {
                let (pre, args, types) = arguments(
                    self,
                    &mut call.args,
                    &expect.into_iter().cloned().collect::<Vec<_>>(),
                )?;
                let ty = expect.cloned().unwrap_or_else(|| types[0].clone());
                let js =
                    if ty == Ty::Str { format!("String({})", args[0]) } else { args[0].clone() };
                Ok(seq(pre, js, ty))
            }
            ("IntoChildren", "extend_into") => {
                let mut args = call.args.iter_mut();
                let (Some(children), Some(target)) = (args.next(), args.next()) else {
                    return Err(());
                };
                let children = self.expr(children, None)?;
                let (mut pre, target, _) = self.place(strip_ref(target))?;
                pre.extend(children.pre);
                pre.push(format!("rn.extendChildren({target}, {});", children.js));
                Ok(seq(pre, "undefined", Ty::Unit))
            }
            _ => self.fail(
                span,
                &format!("`{owner}::{name}`"),
                "the subset does not know this function",
            ),
        }
    }

    // ----------------------------------------------------- method calls ----

    pub fn method_call(&mut self, expr: &mut syn::Expr, expect: Option<&Ty>) -> R<V> {
        let syn::Expr::MethodCall(call) = expr else { return Err(()) };
        let span = call.method.span();
        let method = call.method.to_string();
        // Methods that change a string in place need its place.
        if matches!(
            method.as_str(),
            "push_str" | "push" | "clear" | "insert_str" | "truncate" | "pop"
        ) {
            if let Ok((pre, place, ty)) = self.place_quiet(&mut call.receiver) {
                if ty == Ty::Str {
                    let (more, args, _) = arguments(self, &mut call.args, &[Ty::Str])?;
                    let mut all = pre;
                    all.extend(more);
                    let js = match method.as_str() {
                        "push_str" | "push" => format!("{place} = {place} + {}", args[0]),
                        "clear" => format!("{place} = \"\""),
                        "pop" => {
                            let last = self.temp();
                            all.push(format!("const {last} = [...{place}].pop() ?? null;"));
                            all.push(format!("if ({last} !== null) {place} = {place}.slice(0, {place}.length - {last}.length);"));
                            return Ok(seq(all, last, Ty::Opt(Box::new(Ty::Char))));
                        }
                        _ => {
                            return self.fail(
                                span,
                                &format!("`String::{method}`"),
                                "strings are indexed by bytes in Rust",
                            );
                        }
                    };
                    all.push(format!("{js};"));
                    return Ok(seq(all, "undefined", Ty::Unit));
                }
            }
        }
        if method == "take" || method == "replace" || method == "get_or_insert_with" {
            if let Ok((pre, place, Ty::Opt(inner))) = self.place_quiet(&mut call.receiver) {
                let mut all = pre;
                let taken = self.temp();
                all.push(format!("const {taken} = {place};"));
                if method == "take" {
                    all.push(format!("{place} = null;"));
                } else if method == "replace" {
                    let (more, args, _) = arguments(self, &mut call.args, &[(*inner).clone()])?;
                    all.extend(more);
                    all.push(format!("{place} = {};", args[0]));
                } else {
                    let Some(argument) = call.args.first_mut() else { return Err(()) };
                    let make = closure_arg(self, argument, &[])?;
                    all.extend(make.pre);
                    all.push(format!("if ({place} === null) {place} = {}();", make.js));
                    return Ok(seq(all, place, (*inner).clone()));
                }
                return Ok(seq(all, taken, Ty::Opt(inner)));
            }
        }
        let turbofish: Vec<syn::Type> = call
            .turbofish
            .as_ref()
            .map(|turbofish| {
                turbofish
                    .args
                    .iter()
                    .filter_map(|argument| match argument {
                        syn::GenericArgument::Type(ty) => Some(ty.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let receiver = self.expr(&mut call.receiver, None)?;
        let receiver_ty = receiver.ty.clone();
        match &receiver_ty {
            Ty::Adt(adt) => {
                if let Some(sig) = self.module.methods.get(&(adt.clone(), method.clone())).cloned()
                {
                    let (pre, args, _) = arguments(self, &mut call.args, &sig.params)?;
                    let mut all = receiver.pre;
                    all.extend(pre);
                    let mut list = vec![receiver.js];
                    list.extend(args);
                    return Ok(seq(all, format!("{adt}${method}({})", list.join(", ")), sig.ret));
                }
                match method.as_str() {
                    "clone" => {
                        Ok(seq(receiver.pre, format!("rn.clone({})", receiver.js), receiver_ty))
                    }
                    "eq" | "ne" => {
                        let (pre, args, _) =
                            arguments(self, &mut call.args, std::slice::from_ref(&receiver_ty))?;
                        let mut all = receiver.pre;
                        all.extend(pre);
                        let negate = if method == "ne" { "!" } else { "" };
                        Ok(seq(
                            all,
                            format!("{negate}rn.eq({}, {})", receiver.js, args[0]),
                            Ty::Bool,
                        ))
                    }
                    _ => self.fail(
                        span,
                        &format!("`{adt}::{method}`"),
                        "it is not a method of the client module",
                    ),
                }
            }
            Ty::Effects(_) => self.effect(receiver, &method, &turbofish, call, span),
            _ => self.builtin_method(receiver, &method, &turbofish, call, expect, span),
        }
    }

    /// A place, without reporting when it is not one.
    fn place_quiet(&mut self, expr: &mut syn::Expr) -> R<(Vec<String>, String, Ty)> {
        let errors = self.errors.len();
        let result = self.place(expr);
        self.errors.truncate(errors);
        result
    }

    fn effect(
        &mut self,
        fx: V,
        method: &str,
        turbofish: &[syn::Type],
        call: &mut syn::ExprMethodCall,
        span: Span,
    ) -> R<V> {
        let message = self.module.message.clone();
        let reply = |ty: Ty| vec![Ty::Fn(Box::new(message.clone())), ty];
        let (expected, js_args): (Vec<Ty>, fn(&[String], &str) -> String) = match method {
            "call" => {
                let Some(function) = turbofish.first() else {
                    return self.fail(
                        span,
                        "`fx.call` without the function",
                        "write `fx.call::<F>(input, reply)`",
                    );
                };
                let syn::Type::Path(path) = function else { return Err(()) };
                self.server_fns.push(path.path.clone());
                let name = last_segment(&path.path);
                let (pre, args, _) = arguments(
                    self,
                    &mut call.args,
                    &[
                        Ty::Opaque("Input".into()),
                        Ty::Res(Box::new(Ty::Opaque("Output".into())), Box::new(Ty::ServerFnError)),
                    ],
                )?;
                let mut all = fx.pre;
                all.extend(pre);
                all.push(format!(
                    "{}.call(fns[{}], {}, {});",
                    fx.js,
                    string(&name),
                    args[0],
                    args[1]
                ));
                return Ok(seq(all, "undefined", Ty::Unit));
            }
            "after" => (vec![Ty::Int(IntK::U64), message.clone()], |a, fx| {
                format!("{fx}.after({}, {})", a[0], a[1])
            }),
            "navigate" => (vec![Ty::Str], |a, fx| format!("{fx}.navigate(String({}))", a[0])),
            "back" => (vec![], |_, fx| format!("{fx}.back()")),
            "focus" => (vec![Ty::Str], |a, fx| format!("{fx}.focus(String({}))", a[0])),
            "copy" => (vec![Ty::Str], |a, fx| format!("{fx}.copy(String({}))", a[0])),
            "store" => (vec![Ty::Str, Ty::Opaque("_".into())], |a, fx| {
                format!("{fx}.store(String({}), rn.clone({}))", a[0], a[1])
            }),
            "load" => (
                reply(Ty::Opt(Box::new(Ty::Opaque("_".into())))).into_iter().rev().collect(),
                |a, fx| format!("{fx}.load(String({}), {})", a[0], a[1]),
            ),
            "notify" => (vec![Ty::Str, Ty::Str], |a, fx| {
                format!("{fx}.notify(String({}), String({}))", a[0], a[1])
            }),
            "publish" => (vec![Ty::Str, Ty::Opaque("_".into())], |a, fx| {
                format!("{fx}.publish(String({}), rn.clone({}))", a[0], a[1])
            }),
            "subscribe" => (vec![Ty::Str, Ty::Fn(Box::new(message.clone()))], |a, fx| {
                format!("{fx}.subscribe(String({}), {})", a[0], a[1])
            }),
            "download" => (vec![Ty::Str, Ty::Str], |a, fx| {
                format!("{fx}.download(String({}), String({}))", a[0], a[1])
            }),
            "js" => (
                vec![
                    Ty::Str,
                    Ty::Str,
                    Ty::Opaque("Value".into()),
                    Ty::Fn(Box::new(message.clone())),
                ],
                |a, fx| format!("{fx}.js(String({}), String({}), {}, {})", a[0], a[1], a[2], a[3]),
            ),
            other => {
                if let Some(extra) = crate::methods::capability_effect(other) {
                    let (pre, args, _) = arguments(self, &mut call.args, &[])?;
                    let mut all = fx.pre;
                    all.extend(pre);
                    all.push(format!("{}.{extra}({});", fx.js, args.join(", ")));
                    return Ok(seq(all, "undefined", Ty::Unit));
                }
                return self.fail(span, &format!("`fx.{other}`"), "no such effect");
            }
        };
        let expected: Vec<Ty> =
            if method == "load" { vec![Ty::Str, Ty::Fn(Box::new(message))] } else { expected };
        let (pre, args, _) = arguments(self, &mut call.args, &expected)?;
        let mut all = fx.pre;
        all.extend(pre);
        all.push(format!("{};", js_args(&args, &fx.js)));
        Ok(seq(all, "undefined", Ty::Unit))
    }

    #[allow(clippy::cognitive_complexity, reason = "one table per receiver type")]
    fn builtin_method(
        &mut self,
        receiver: V,
        method: &str,
        turbofish: &[syn::Type],
        call: &mut syn::ExprMethodCall,
        expect: Option<&Ty>,
        span: Span,
    ) -> R<V> {
        let V { pre: mut all, js: r, ty } = receiver;
        let r = paren(&r);
        let unknown = |cx: &mut Self, ty: &Ty| -> R<V> {
            cx.fail(span, &format!("`.{method}()` on {ty}"), "the subset does not know this method")
        };
        macro_rules! args {
            ($($ty:expr),*) => {{
                let expected = [$($ty),*];
                let (pre, args, types) = arguments(self, &mut call.args, &expected)?;
                all.extend(pre);
                (args, types)
            }};
        }
        macro_rules! done {
            ($js:expr, $ty:expr) => {
                return Ok(seq(all, $js, $ty))
            };
        }
        // Methods every value has.
        match method {
            "clone" | "to_owned" | "cloned" | "copied"
                if !matches!(ty, Ty::Iter(_) | Ty::Opt(_)) =>
            {
                if ty.is_primitive() {
                    done!(r, ty);
                }
                done!(format!("rn.clone({r})"), ty);
            }
            "as_str" | "as_ref" | "as_mut" | "borrow" | "as_deref" | "into"
                if !matches!(ty, Ty::Opt(_) | Ty::Res(..)) =>
            {
                let target = if method == "into" {
                    expect.cloned().unwrap_or(ty.clone())
                } else {
                    ty.clone()
                };
                if method == "into" && target == Ty::Str {
                    done!(format!("String({r})"), Ty::Str);
                }
                done!(r, target);
            }
            "to_string" => {
                let js = match &ty {
                    Ty::Str | Ty::Char | Ty::NodeId => r.clone(),
                    Ty::Int(_) => format!("String({r})"),
                    Ty::Float(f32) => {
                        format!("rn.float({r}, {})", if *f32 { "\"f32\"" } else { "\"f64\"" })
                    }
                    Ty::Bool => format!("String({r})"),
                    Ty::ServerFnError | Ty::ParseError => format!("rn.errorText({r})"),
                    Ty::Date => format!("rn.dateText({r})"),
                    Ty::Opaque(_) => format!("rn.display({r}, \"any\")"),
                    other => {
                        return self.fail(
                            span,
                            &format!("`.to_string()` on {other}"),
                            "it has no `Display` the subset knows",
                        );
                    }
                };
                done!(js, Ty::Str);
            }
            "eq" | "ne" => {
                let (args, _) = args!(ty.clone());
                let js = if ty.is_primitive() {
                    format!("({r} === {})", args[0])
                } else {
                    format!("rn.eq({r}, {})", args[0])
                };
                done!(if method == "ne" { format!("!{js}") } else { js }, Ty::Bool);
            }
            "cmp" | "partial_cmp" if ty.is_numeric() || ty == Ty::Str || ty == Ty::Char => {
                let (args, _) = args!(ty.clone());
                done!(format!("rn.cmp({r}, {})", args[0]), Ty::Opaque("Ordering".into()));
            }
            _ => {}
        }
        match ty.clone() {
            Ty::Str => match method {
                "len" => done!(format!("rn.utf8len({r})"), USIZE),
                "is_empty" => done!(format!("({r} === \"\")"), Ty::Bool),
                "trim" => done!(format!("rn.trim({r})"), Ty::Str),
                "trim_start" => done!(format!("rn.trimStart({r})"), Ty::Str),
                "trim_end" => done!(format!("rn.trimEnd({r})"), Ty::Str),
                "to_uppercase" => done!(format!("{r}.toUpperCase()"), Ty::Str),
                "to_lowercase" => done!(format!("{r}.toLowerCase()"), Ty::Str),
                "to_ascii_uppercase" => done!(format!("rn.asciiUpper({r})"), Ty::Str),
                "to_ascii_lowercase" => done!(format!("rn.asciiLower({r})"), Ty::Str),
                "contains" | "starts_with" | "ends_with" => {
                    let (args, _) = args!(Ty::Str);
                    let js = match method {
                        "contains" => "includes",
                        "starts_with" => "startsWith",
                        _ => "endsWith",
                    };
                    done!(format!("{r}.{js}({})", args[0]), Ty::Bool);
                }
                "eq_ignore_ascii_case" => {
                    let (args, _) = args!(Ty::Str);
                    done!(format!("(rn.asciiLower({r}) === rn.asciiLower({}))", args[0]), Ty::Bool);
                }
                "replace" => {
                    let (args, _) = args!(Ty::Str, Ty::Str);
                    done!(format!("rn.replaceAll({r}, {}, {})", args[0], args[1]), Ty::Str);
                }
                "repeat" => {
                    let (args, _) = args!(USIZE);
                    done!(format!("{r}.repeat({})", args[0]), Ty::Str);
                }
                "split" => {
                    let (args, _) = args!(Ty::Str);
                    done!(format!("rn.split({r}, {})", args[0]), Ty::Iter(Box::new(Ty::Str)));
                }
                "split_whitespace" => done!(format!("rn.splitWhitespace({r})"), Ty::Iter(Box::new(Ty::Str))),
                "lines" => done!(format!("rn.lines({r})"), Ty::Iter(Box::new(Ty::Str))),
                "chars" => done!(format!("[...{r}]"), Ty::Iter(Box::new(Ty::Char))),
                "parse" => {
                    let target = turbofish.first().map(|ty| self.module.read(ty)).or_else(|| match expect {
                        Some(Ty::Res(ok, _)) => Some((**ok).clone()),
                        _ => None,
                    });
                    let js = match &target {
                        Some(Ty::Int(kind)) => format!("rn.parseInt_({r}, {})", self.kind_js(*kind)),
                        Some(Ty::Float(f32)) => format!("rn.parseFloat_({r}, {})", if *f32 { "\"f32\"" } else { "\"f64\"" }),
                        Some(Ty::Bool) => format!("rn.parseBool({r})"),
                        _ => return self.fail(span, "`.parse()` without its type", "write `.parse::<i32>()` (numbers and `bool`)"),
                    };
                    done!(js, Ty::Res(Box::new(target.unwrap_or(Ty::Unit)), Box::new(Ty::ParseError)));
                }
                _ => unknown(self, &ty),
            },
            Ty::Char => match method {
                "is_whitespace" => done!(format!("rn.isWhitespace({r})"), Ty::Bool),
                "is_alphabetic" => done!(format!("rn.isAlphabetic({r})"), Ty::Bool),
                "is_numeric" => done!(format!("rn.isNumeric({r})"), Ty::Bool),
                "is_alphanumeric" => done!(format!("rn.isAlphanumeric({r})"), Ty::Bool),
                "is_uppercase" => done!(format!("/^\\p{{Uppercase}}$/u.test({r})"), Ty::Bool),
                "is_lowercase" => done!(format!("/^\\p{{Lowercase}}$/u.test({r})"), Ty::Bool),
                "is_ascii_digit" => done!(format!("/^[0-9]$/.test({r})"), Ty::Bool),
                "is_ascii_alphabetic" => done!(format!("/^[A-Za-z]$/.test({r})"), Ty::Bool),
                "is_ascii_uppercase" => done!(format!("/^[A-Z]$/.test({r})"), Ty::Bool),
                "is_ascii_lowercase" => done!(format!("/^[a-z]$/.test({r})"), Ty::Bool),
                "is_ascii_punctuation" => done!(format!("/^[!-/:-@\\[-`{{-~]$/.test({r})"), Ty::Bool),
                "to_ascii_uppercase" => done!(format!("rn.asciiUpper({r})"), Ty::Char),
                "to_ascii_lowercase" => done!(format!("rn.asciiLower({r})"), Ty::Char),
                "to_digit" => {
                    let (args, _) = args!(Ty::Int(IntK::U32));
                    done!(format!("rn.toDigit({r}, {})", args[0]), Ty::Opt(Box::new(Ty::Int(IntK::U32))));
                }
                "len_utf8" => done!(format!("rn.utf8len({r})"), USIZE),
                _ => unknown(self, &ty),
            },
            Ty::Int(kind) => {
                let k = self.kind_js(kind);
                match method {
                    "abs" => done!(format!("rn.neg({r} < 0 ? {r} : -{r}, {k})"), Ty::Int(kind)),
                    "pow" => {
                        let (args, _) = args!(Ty::Int(IntK::U32));
                        done!(format!("rn.pow({r}, {}, {k})", args[0]), Ty::Int(kind));
                    }
                    "min" | "max" => {
                        let (args, _) = args!(Ty::Int(kind));
                        done!(format!("Math.{method}({r}, {})", args[0]), Ty::Int(kind));
                    }
                    "clamp" => {
                        let (args, _) = args!(Ty::Int(kind), Ty::Int(kind));
                        done!(format!("Math.min(Math.max({r}, {}), {})", args[0], args[1]), Ty::Int(kind));
                    }
                    "signum" => done!(format!("Math.sign({r})"), Ty::Int(kind)),
                    "is_positive" => done!(format!("({r} > 0)"), Ty::Bool),
                    "is_negative" => done!(format!("({r} < 0)"), Ty::Bool),
                    "checked_add" | "checked_sub" | "checked_mul" | "checked_div" | "checked_rem" => {
                        let (args, _) = args!(Ty::Int(kind));
                        let operation = &method[8..];
                        done!(format!("rn.checked(\"{operation}\", {r}, {}, {k})", args[0]), Ty::Opt(Box::new(Ty::Int(kind))));
                    }
                    "saturating_add" | "saturating_sub" | "saturating_mul" => {
                        let (args, _) = args!(Ty::Int(kind));
                        let operation = &method[11..];
                        done!(format!("rn.saturating(\"{operation}\", {r}, {}, {k})", args[0]), Ty::Int(kind));
                    }
                    "rem_euclid" => {
                        let (args, _) = args!(Ty::Int(kind));
                        done!(format!("rn.remEuclid({r}, {}, {k})", args[0]), Ty::Int(kind));
                    }
                    "abs_diff" => {
                        let (args, _) = args!(Ty::Int(kind));
                        done!(format!("Math.abs({r} - {})", args[0]), Ty::Int(kind));
                    }
                    "is_power_of_two" => done!(format!("({r} > 0 && ({r} & ({r} - 1)) === 0)"), Ty::Bool),
                    _ => unknown(self, &ty),
                }
            }
            Ty::Float(f32) => {
                let wrap = |js: String| if f32 { format!("Math.fround({js})") } else { js };
                let unary = |name: &str| -> Option<&'static str> {
                    Some(match name {
                        "abs" => "Math.abs",
                        "floor" => "Math.floor",
                        "ceil" => "Math.ceil",
                        "trunc" => "Math.trunc",
                        "sqrt" => "Math.sqrt",
                        "sin" => "Math.sin",
                        "cos" => "Math.cos",
                        "tan" => "Math.tan",
                        "asin" => "Math.asin",
                        "acos" => "Math.acos",
                        "atan" => "Math.atan",
                        "exp" => "Math.exp",
                        "ln" => "Math.log",
                        "log10" => "Math.log10",
                        "log2" => "Math.log2",
                        "cbrt" => "Math.cbrt",
                        "round" => "rn.round",
                        "signum" => "rn.signum",
                        "fract" => "rn.fract",
                        _ => return None,
                    })
                };
                if let Some(function) = unary(method) {
                    done!(wrap(format!("{function}({r})")), Ty::Float(f32));
                }
                match method {
                    "powi" | "powf" | "min" | "max" | "atan2" | "hypot" => {
                        let expected = if method == "powi" { Ty::Int(IntK::I32) } else { Ty::Float(f32) };
                        let (args, _) = args!(expected);
                        let function = match method {
                            "powi" | "powf" => "Math.pow",
                            "min" => "rn.fmin",
                            "max" => "rn.fmax",
                            "atan2" => "Math.atan2",
                            _ => "Math.hypot",
                        };
                        done!(wrap(format!("{function}({r}, {})", args[0])), Ty::Float(f32));
                    }
                    "clamp" => {
                        let (args, _) = args!(Ty::Float(f32), Ty::Float(f32));
                        done!(format!("Math.min(Math.max({r}, {}), {})", args[0], args[1]), Ty::Float(f32));
                    }
                    "is_nan" => done!(format!("Number.isNaN({r})"), Ty::Bool),
                    "is_finite" => done!(format!("Number.isFinite({r})"), Ty::Bool),
                    "is_infinite" => done!(format!("(Math.abs({r}) === Infinity)"), Ty::Bool),
                    "is_sign_negative" => done!(format!("({r} < 0 || Object.is({r}, -0))"), Ty::Bool),
                    "to_degrees" => done!(wrap(format!("({r} * 180 / Math.PI)")), Ty::Float(f32)),
                    "to_radians" => done!(wrap(format!("({r} * Math.PI / 180)")), Ty::Float(f32)),
                    _ => unknown(self, &ty),
                }
            }
            Ty::Bool => match method {
                "then_some" => {
                    let (args, types) = args!(Ty::Opaque("_".into()));
                    done!(format!("({r} ? {} : null)", args[0]), Ty::Opt(Box::new(types[0].clone())));
                }
                _ => unknown(self, &ty),
            },
            Ty::NodeId => match method {
                "local_key" => done!(r, Ty::Opt(Box::new(Ty::Str))),
                _ => unknown(self, &ty),
            },
            Ty::Date => unknown(self, &ty),
            Ty::ServerFnError | Ty::ParseError => unknown(self, &ty),
            Ty::Opt(inner) => {
                let inner = *inner;
                match method {
                    "is_some" => done!(format!("({r} !== null)"), Ty::Bool),
                    "is_none" => done!(format!("({r} === null)"), Ty::Bool),
                    "unwrap" => done!(format!("rn.unwrap({r})"), inner),
                    "expect" => {
                        let (args, _) = args!(Ty::Str);
                        done!(format!("rn.unwrap({r}, {})", args[0]), inner);
                    }
                    "unwrap_or" => {
                        let (args, _) = args!(inner.clone());
                        done!(format!("rn.or({r}, {})", args[0]), inner);
                    }
                    "unwrap_or_default" => {
                        let default = match self.module.default_js(&inner, span) {
                            Ok(js) => js,
                            Err(error) => {
                                self.error(error);
                                return Err(());
                            }
                        };
                        done!(format!("rn.or({r}, {default})"), inner);
                    }
                    "unwrap_or_else" => {
                        let Some(argument) = call.args.first_mut() else { return Err(()) };
                        let make = closure_arg(self, argument, &[])?;
                        all.extend(make.pre);
                        done!(format!("rn.orElse({r}, {})", make.js), inner);
                    }
                    "map" | "and_then" | "filter" | "is_some_and" | "is_none_or" | "map_or" | "map_or_else" | "ok_or" | "ok_or_else" | "or" | "or_else" | "xor" | "zip" => {
                        self.option_combinator(all, &r, inner, method, call, span)
                    }
                    "as_ref" | "as_mut" | "as_deref" | "as_deref_mut" | "copied" | "cloned" => {
                        let js = if method == "cloned" && !inner.is_primitive() { format!("rn.clone({r})") } else { r };
                        done!(js, Ty::Opt(Box::new(inner)));
                    }
                    "iter" | "into_iter" => done!(format!("({r} === null ? [] : [{r}])"), Ty::Iter(Box::new(inner))),
                    _ => unknown(self, &Ty::Opt(Box::new(inner))),
                }
            }
            Ty::Res(ok, err) => {
                let (ok, err) = (*ok, *err);
                match method {
                    "is_ok" => done!(format!("(\"Ok\" in {r})"), Ty::Bool),
                    "is_err" => done!(format!("(\"Err\" in {r})"), Ty::Bool),
                    "ok" => done!(format!("(\"Ok\" in {r} ? {r}.Ok : null)"), Ty::Opt(Box::new(ok))),
                    "err" => done!(format!("(\"Err\" in {r} ? {r}.Err : null)"), Ty::Opt(Box::new(err))),
                    "unwrap" => done!(format!("rn.unwrapOk({r})"), ok),
                    "expect" => {
                        let (args, _) = args!(Ty::Str);
                        done!(format!("rn.unwrapOk({r}, {})", args[0]), ok);
                    }
                    "unwrap_or" => {
                        let (args, _) = args!(ok.clone());
                        done!(format!("(\"Ok\" in {r} ? {r}.Ok : {})", args[0]), ok);
                    }
                    "unwrap_or_default" => {
                        let default = match self.module.default_js(&ok, span) {
                            Ok(js) => js,
                            Err(error) => {
                                self.error(error);
                                return Err(());
                            }
                        };
                        done!(format!("(\"Ok\" in {r} ? {r}.Ok : {default})"), ok);
                    }
                    "unwrap_or_else" => {
                        let Some(argument) = call.args.first_mut() else { return Err(()) };
                        let make = closure_arg(self, argument, &[err])?;
                        all.extend(make.pre);
                        done!(format!("(\"Ok\" in {r} ? {r}.Ok : {}({r}.Err))", make.js), ok);
                    }
                    "map" | "map_err" | "and_then" | "is_ok_and" | "is_err_and" => {
                        let Some(argument) = call.args.first_mut() else { return Err(()) };
                        let parameter = if method == "map_err" || method == "is_err_and" { err.clone() } else { ok.clone() };
                        let function = closure_arg(self, argument, &[parameter])?;
                        all.extend(function.pre);
                        let out = result_of(&function.ty);
                        let f = function.js;
                        match method {
                            "map" => done!(format!("(\"Ok\" in {r} ? {{ Ok: {f}({r}.Ok) }} : {r})"), Ty::Res(Box::new(out), Box::new(err))),
                            "map_err" => done!(format!("(\"Err\" in {r} ? {{ Err: {f}({r}.Err) }} : {r})"), Ty::Res(Box::new(ok), Box::new(out))),
                            "and_then" => done!(format!("(\"Ok\" in {r} ? {f}({r}.Ok) : {r})"), out),
                            "is_ok_and" => done!(format!("(\"Ok\" in {r} && {f}({r}.Ok))"), Ty::Bool),
                            _ => done!(format!("(\"Err\" in {r} && {f}({r}.Err))"), Ty::Bool),
                        }
                    }
                    _ => unknown(self, &Ty::Res(Box::new(ok), Box::new(err))),
                }
            }
            Ty::Vec(element) => self.vec_method(all, &r, *element, method, turbofish, call, expect, span),
            Ty::Iter(element) => self.iter_method(all, &r, *element, method, turbofish, call, expect, span),
            Ty::Tuple(_) => unknown(self, &ty),
            Ty::Node => self.node_method(all, &r, method, call, span),
            Ty::Layout | Ty::Container | Ty::Grid | Ty::Constraints | Ty::VirtualList | Ty::A11y | Ty::Visual | Ty::Placement => {
                self.builder_method(all, &r, &ty, method, call, span)
            }
            Ty::Opaque(_) => self.fail(span, &format!("`.{method}()` on data from outside the module"), "the subset cannot see its type; bind it with a type annotation or declare the type in the client module"),
            other => unknown(self, &other),
        }
    }

    fn option_combinator(
        &mut self,
        mut all: Vec<String>,
        r: &str,
        inner: Ty,
        method: &str,
        call: &mut syn::ExprMethodCall,
        span: Span,
    ) -> R<V> {
        let mut args = call.args.iter_mut();
        match method {
            "map" | "and_then" | "filter" | "is_some_and" | "is_none_or" => {
                let Some(argument) = args.next() else { return Err(()) };
                let function = closure_arg(self, argument, std::slice::from_ref(&inner))?;
                all.extend(function.pre);
                let f = function.js;
                let out = result_of(&function.ty);
                Ok(match method {
                    "map" => seq(
                        all,
                        format!("({r} === null ? null : {f}({r}))"),
                        Ty::Opt(Box::new(out)),
                    ),
                    "and_then" => seq(all, format!("({r} === null ? null : {f}({r}))"), out),
                    "filter" => seq(
                        all,
                        format!("({r} !== null && {f}({r}) ? {r} : null)"),
                        Ty::Opt(Box::new(inner)),
                    ),
                    "is_some_and" => seq(all, format!("({r} !== null && {f}({r}))"), Ty::Bool),
                    _ => seq(all, format!("({r} === null || {f}({r}))"), Ty::Bool),
                })
            }
            "map_or" | "map_or_else" => {
                let (Some(default), Some(function)) = (args.next(), args.next()) else {
                    return Err(());
                };
                let default = if method == "map_or" {
                    self.expr(default, None)?
                } else {
                    closure_arg(self, default, &[])?
                };
                let function = closure_arg(self, function, &[inner])?;
                all.extend(default.pre);
                all.extend(function.pre);
                let fallback =
                    if method == "map_or" { default.js } else { format!("{}()", default.js) };
                Ok(seq(
                    all,
                    format!("({r} === null ? {fallback} : {}({r}))", function.js),
                    result_of(&function.ty),
                ))
            }
            "ok_or" | "ok_or_else" => {
                let Some(argument) = args.next() else { return Err(()) };
                let error = if method == "ok_or" {
                    self.expr(argument, None)?
                } else {
                    closure_arg(self, argument, &[])?
                };
                all.extend(error.pre);
                let value = if method == "ok_or" { error.js } else { format!("{}()", error.js) };
                let error_ty = if method == "ok_or" { error.ty } else { result_of(&error.ty) };
                Ok(seq(
                    all,
                    format!("({r} === null ? {{ Err: {value} }} : {{ Ok: {r} }})"),
                    Ty::Res(Box::new(inner), Box::new(error_ty)),
                ))
            }
            "or" | "or_else" | "xor" | "zip" => {
                let Some(argument) = args.next() else { return Err(()) };
                let other = if method == "or_else" {
                    closure_arg(self, argument, &[])?
                } else {
                    self.expr(argument, None)?
                };
                all.extend(other.pre);
                let o = if method == "or_else" { format!("{}()", other.js) } else { other.js };
                Ok(match method {
                    "or" | "or_else" => {
                        seq(all, format!("({r} !== null ? {r} : {o})"), Ty::Opt(Box::new(inner)))
                    }
                    "xor" => seq(
                        all,
                        format!("(({r} === null) !== ({o} === null) ? ({r} ?? {o}) : null)"),
                        Ty::Opt(Box::new(inner)),
                    ),
                    _ => seq(
                        all,
                        format!("({r} !== null && {o} !== null ? [{r}, {o}] : null)"),
                        Ty::Opt(Box::new(Ty::Tuple(vec![inner, Ty::Opaque("_".into())]))),
                    ),
                })
            }
            _ => self.fail(span, &format!("`Option::{method}`"), "not in the subset"),
        }
    }

    #[allow(clippy::too_many_arguments, reason = "a method's receiver, name, and call site")]
    fn vec_method(
        &mut self,
        mut all: Vec<String>,
        r: &str,
        element: Ty,
        method: &str,
        turbofish: &[syn::Type],
        call: &mut syn::ExprMethodCall,
        expect: Option<&Ty>,
        span: Span,
    ) -> R<V> {
        let vec = Ty::Vec(Box::new(element.clone()));
        macro_rules! args {
            ($($ty:expr),*) => {{
                let expected = [$($ty),*];
                let (pre, args, types) = arguments(self, &mut call.args, &expected)?;
                all.extend(pre);
                (args, types)
            }};
        }
        let eq = |element: &Ty, a: &str, b: &str| {
            if element.is_primitive() { format!("{a} === {b}") } else { format!("rn.eq({a}, {b})") }
        };
        Ok(match method {
            "len" => seq(all, format!("{r}.length"), USIZE),
            "is_empty" => seq(all, format!("({r}.length === 0)"), Ty::Bool),
            "push" => {
                let (args, types) = args!(element.clone());
                let copied = if self.module.is_copy(&types[0]) {
                    format!("rn.clone({})", args[0])
                } else {
                    args[0].clone()
                };
                all.push(format!("{r}.push({copied});"));
                seq(all, "undefined", Ty::Unit)
            }
            "pop" => {
                seq(all, format!("({r}.length ? {r}.pop() : null)"), Ty::Opt(Box::new(element)))
            }
            "insert" => {
                let (args, _) = args!(USIZE, element.clone());
                all.push(format!("rn.vecInsert({r}, {}, {});", args[0], args[1]));
                seq(all, "undefined", Ty::Unit)
            }
            "remove" => {
                let (args, _) = args!(USIZE);
                seq(all, format!("rn.vecRemove({r}, {})", args[0]), element)
            }
            "swap_remove" => {
                let (args, _) = args!(USIZE);
                seq(all, format!("rn.swapRemove({r}, {})", args[0]), element)
            }
            "clear" => {
                all.push(format!("{r}.length = 0;"));
                seq(all, "undefined", Ty::Unit)
            }
            "truncate" => {
                let (args, _) = args!(USIZE);
                all.push(format!("if ({r}.length > {0}) {r}.length = {0};", args[0]));
                seq(all, "undefined", Ty::Unit)
            }
            "contains" => {
                let (args, _) = args!(element.clone());
                let test = eq(&element, "x", &args[0]);
                seq(all, format!("{r}.some((x) => {test})"), Ty::Bool)
            }
            "get" => {
                let (args, _) = args!(USIZE);
                seq(
                    all,
                    format!("({0} < {r}.length ? {r}[{0}] : null)", args[0]),
                    Ty::Opt(Box::new(element)),
                )
            }
            "get_mut" => {
                let (args, _) = args!(USIZE);
                seq(
                    all,
                    format!("({0} < {r}.length ? {r}[{0}] : null)", args[0]),
                    Ty::Opt(Box::new(element)),
                )
            }
            "first" => {
                seq(all, format!("({r}.length ? {r}[0] : null)"), Ty::Opt(Box::new(element)))
            }
            "last" => seq(
                all,
                format!("({r}.length ? {r}[{r}.length - 1] : null)"),
                Ty::Opt(Box::new(element)),
            ),
            "sort" | "sort_unstable" => {
                all.push(format!("{r}.sort(rn.cmp);"));
                seq(all, "undefined", Ty::Unit)
            }
            "sort_by_key" | "sort_unstable_by_key" | "sort_by_cached_key" => {
                let Some(argument) = call.args.first_mut() else { return Err(()) };
                let key = closure_arg(self, argument, &[element])?;
                all.extend(key.pre);
                all.push(format!(
                    "{{ const k = {}; {r}.sort((a, b) => rn.cmp(k(a), k(b))); }}",
                    key.js
                ));
                seq(all, "undefined", Ty::Unit)
            }
            "sort_by" | "sort_unstable_by" => {
                let Some(argument) = call.args.first_mut() else { return Err(()) };
                let compare = closure_arg(self, argument, &[element.clone(), element])?;
                all.extend(compare.pre);
                all.push(format!("{r}.sort({});", compare.js));
                seq(all, "undefined", Ty::Unit)
            }
            "reverse" => {
                all.push(format!("{r}.reverse();"));
                seq(all, "undefined", Ty::Unit)
            }
            "retain" => {
                let Some(argument) = call.args.first_mut() else { return Err(()) };
                let keep = closure_arg(self, argument, &[element])?;
                all.extend(keep.pre);
                all.push(format!("rn.retain({r}, {});", keep.js));
                seq(all, "undefined", Ty::Unit)
            }
            "dedup" => {
                all.push(format!("rn.dedup({r});"));
                seq(all, "undefined", Ty::Unit)
            }
            "extend" | "append" | "extend_from_slice" => {
                let (args, _) = args!(Ty::Vec(Box::new(element)));
                all.push(format!("{r}.push(...rn.clone([...{}]));", args[0]));
                if method == "append" {
                    all.push(format!("{}.length = 0;", args[0]));
                }
                seq(all, "undefined", Ty::Unit)
            }
            "swap" => {
                let (args, _) = args!(USIZE, USIZE);
                all.push(format!("rn.swap({r}, {}, {});", args[0], args[1]));
                seq(all, "undefined", Ty::Unit)
            }
            "join" | "concat" if element == Ty::Str => {
                let (args, _) = if method == "join" {
                    args!(Ty::Str)
                } else {
                    (vec!["\"\"".to_owned()], Vec::new())
                };
                seq(all, format!("{r}.join({})", args[0]), Ty::Str)
            }
            "to_vec" | "into_vec" => seq(all, format!("rn.clone({r})"), vec),
            "iter" | "iter_mut" | "into_iter" | "drain" => {
                if method == "drain" {
                    let taken = self.temp();
                    all.push(format!("const {taken} = {r}.splice(0);"));
                    return Ok(seq(all, taken, Ty::Iter(Box::new(element))));
                }
                seq(all, format!("[...{r}]"), Ty::Iter(Box::new(element)))
            }
            _ => {
                return self.iter_method(
                    all,
                    &format!("[...{r}]"),
                    element,
                    method,
                    turbofish,
                    call,
                    expect,
                    span,
                );
            }
        })
    }

    #[allow(clippy::too_many_arguments, reason = "a method's receiver, name, and call site")]
    fn iter_method(
        &mut self,
        mut all: Vec<String>,
        r: &str,
        element: Ty,
        method: &str,
        turbofish: &[syn::Type],
        call: &mut syn::ExprMethodCall,
        expect: Option<&Ty>,
        span: Span,
    ) -> R<V> {
        let iter = |ty: Ty| Ty::Iter(Box::new(ty));
        let mut function = |cx: &mut Self, params: &[Ty], all: &mut Vec<String>| -> R<V> {
            let Some(argument) = call.args.first_mut() else { return Err(()) };
            let value = closure_arg(cx, argument, params)?;
            all.extend(value.pre.clone());
            Ok(value)
        };
        Ok(match method {
            "map" => {
                let f = function(self, &[element], &mut all)?;
                seq(all, format!("{r}.map({})", f.js), iter(result_of(&f.ty)))
            }
            "filter" => {
                let f = function(self, std::slice::from_ref(&element), &mut all)?;
                seq(all, format!("{r}.filter({})", f.js), iter(element))
            }
            "filter_map" => {
                let f = function(self, &[element], &mut all)?;
                let out = match result_of(&f.ty) {
                    Ty::Opt(inner) => *inner,
                    other => other,
                };
                seq(all, format!("{r}.map({}).filter((x) => x !== null)", f.js), iter(out))
            }
            "flat_map" | "flatten" => {
                if method == "flatten" {
                    let inner = element.element().unwrap_or(Ty::Opaque("_".into()));
                    return Ok(seq(
                        all,
                        format!(
                            "{r}.flatMap((x) => (x === null ? [] : Array.isArray(x) ? x : [x]))"
                        ),
                        iter(inner),
                    ));
                }
                let f = function(self, &[element], &mut all)?;
                let out = result_of(&f.ty).element().unwrap_or(Ty::Opaque("_".into()));
                seq(
                    all,
                    format!(
                        "{r}.flatMap((x) => {{ const y = ({})(x); return y === null ? [] : Array.isArray(y) ? y : [y]; }})",
                        f.js
                    ),
                    iter(out),
                )
            }
            "enumerate" => seq(
                all,
                format!("{r}.map((x, i) => [i, x])"),
                iter(Ty::Tuple(vec![USIZE, element])),
            ),
            "rev" => seq(all, format!("[...{r}].reverse()"), iter(element)),
            "skip" | "take" => {
                let (pre, args, _) = arguments(self, &mut call.args, &[USIZE])?;
                all.extend(pre);
                let js = if method == "skip" {
                    format!("{r}.slice({})", args[0])
                } else {
                    format!("{r}.slice(0, {})", args[0])
                };
                seq(all, js, iter(element))
            }
            "step_by" => {
                let (pre, args, _) = arguments(self, &mut call.args, &[USIZE])?;
                all.extend(pre);
                seq(all, format!("{r}.filter((_, i) => i % {} === 0)", args[0]), iter(element))
            }
            "chain" => {
                let (pre, args, _) = arguments(self, &mut call.args, &[iter(element.clone())])?;
                all.extend(pre);
                seq(all, format!("[...{r}, ...{}]", args[0]), iter(element))
            }
            "zip" => {
                let (pre, args, types) = arguments(self, &mut call.args, &[])?;
                all.extend(pre);
                let other = types[0].element().unwrap_or(Ty::Opaque("_".into()));
                seq(
                    all,
                    format!("rn.zip({r}, [...{}])", args[0]),
                    iter(Ty::Tuple(vec![element, other])),
                )
            }
            "count" => seq(all, format!("{r}.length"), USIZE),
            "sum" | "product" => {
                let js = match &element {
                    Ty::Int(kind) => format!("rn.{method}({r}, {})", self.kind_js(*kind)),
                    _ => format!("rn.{method}({r}, null)"),
                };
                let ty = match (&element, expect) {
                    (Ty::Opaque(_), Some(expect)) => expect.clone(),
                    _ => element,
                };
                seq(all, js, ty)
            }
            "min" | "max" => seq(
                all,
                format!("rn.minMax({r}, {})", if method == "max" { 1 } else { -1 }),
                Ty::Opt(Box::new(element)),
            ),
            "min_by_key" | "max_by_key" => {
                let f = function(self, std::slice::from_ref(&element), &mut all)?;
                seq(
                    all,
                    format!(
                        "rn.byKey({r}, {}, {})",
                        f.js,
                        if method == "max_by_key" { 1 } else { -1 }
                    ),
                    Ty::Opt(Box::new(element)),
                )
            }
            "any" | "all" => {
                let f = function(self, &[element], &mut all)?;
                seq(
                    all,
                    format!("{r}.{}({})", if method == "any" { "some" } else { "every" }, f.js),
                    Ty::Bool,
                )
            }
            "find" => {
                let f = function(self, std::slice::from_ref(&element), &mut all)?;
                seq(all, format!("({r}.find({}) ?? null)", f.js), Ty::Opt(Box::new(element)))
            }
            "position" => {
                let f = function(self, &[element], &mut all)?;
                seq(all, format!("rn.position({r}, {})", f.js), Ty::Opt(Box::new(USIZE)))
            }
            "last" => seq(all, format!("rn.lastOf({r})"), Ty::Opt(Box::new(element))),
            "nth" => {
                let (pre, args, _) = arguments(self, &mut call.args, &[USIZE])?;
                all.extend(pre);
                seq(
                    all,
                    format!("({0} < {r}.length ? {r}[{0}] : null)", args[0]),
                    Ty::Opt(Box::new(element)),
                )
            }
            "fold" => {
                let mut args = call.args.iter_mut();
                let (Some(initial), Some(step)) = (args.next(), args.next()) else {
                    return Err(());
                };
                let initial = self.expr(initial, expect)?;
                let step = closure_arg(self, step, &[initial.ty.clone(), element])?;
                all.extend(initial.pre);
                all.extend(step.pre);
                seq(all, format!("{r}.reduce({}, {})", step.js, initial.js), initial.ty)
            }
            "for_each" => {
                let f = function(self, &[element], &mut all)?;
                all.push(format!("{r}.forEach({});", f.js));
                seq(all, "undefined", Ty::Unit)
            }
            "cloned" | "copied" => {
                if element.is_primitive() {
                    seq(all, r.to_owned(), iter(element))
                } else {
                    seq(all, format!("rn.clone({r})"), iter(element))
                }
            }
            "iter" | "into_iter" | "by_ref" | "peekable" => seq(all, r.to_owned(), iter(element)),
            "collect" => {
                let target =
                    turbofish.first().map(|ty| self.module.read(ty)).or_else(|| expect.cloned());
                match target {
                    Some(Ty::Str) => seq(all, format!("{r}.join(\"\")"), Ty::Str),
                    Some(Ty::Vec(inner)) => {
                        let inner = if matches!(*inner, Ty::Opaque(_)) { element } else { *inner };
                        seq(all, r.to_owned(), Ty::Vec(Box::new(inner)))
                    }
                    Some(Ty::Opt(_)) | Some(Ty::Res(..)) => {
                        return self.fail(
                            span,
                            "collecting into an `Option` or `Result`",
                            "collect into a `Vec` and check it",
                        );
                    }
                    _ => seq(all, r.to_owned(), Ty::Vec(Box::new(element))),
                }
            }
            "len" => seq(all, format!("{r}.length"), USIZE),
            "is_empty" => seq(all, format!("({r}.length === 0)"), Ty::Bool),
            "join" => {
                let (pre, args, _) = arguments(self, &mut call.args, &[Ty::Str])?;
                all.extend(pre);
                seq(all, format!("{r}.join({})", args[0]), Ty::Str)
            }
            _ => {
                return self.fail(
                    span,
                    &format!("`.{method}()` on {}", Ty::Iter(Box::new(element))),
                    "not in the subset",
                );
            }
        })
    }

    fn node_method(
        &mut self,
        mut all: Vec<String>,
        r: &str,
        method: &str,
        call: &mut syn::ExprMethodCall,
        span: Span,
    ) -> R<V> {
        let (expected, function): (Vec<Ty>, &str) = match method {
            "disabled" => (vec![Ty::Bool], "disabled"),
            "hidden" => (vec![Ty::Bool], "hidden"),
            "with_class" | "with_declarations" => (vec![Ty::Decl], "withClass"),
            "with_accessibility" => (vec![Ty::A11y], "withAccessibility"),
            "with_style" => (vec![Ty::Visual], "withStyle"),
            "with_state_style" => (vec![Ty::ControlState, Ty::Visual], "withStateStyle"),
            "with_opacity" => (vec![Ty::Float(true)], "withOpacity"),
            "with_cursor" => (vec![Ty::Cursor], "withCursor"),
            "with_item_index" => (vec![USIZE], "withItemIndex"),
            "with_shared_id" => (vec![Ty::Str], "withSharedId"),
            other => {
                return self.fail(
                    span,
                    &format!("`Node::{other}`"),
                    "the subset does not know this modifier",
                );
            }
        };
        let (pre, args, _) = arguments(self, &mut call.args, &expected)?;
        all.extend(pre);
        let mut list = vec![r.to_owned()];
        list.extend(args);
        Ok(seq(all, format!("rn.{function}({})", list.join(", ")), Ty::Node))
    }

    fn builder_method(
        &mut self,
        mut all: Vec<String>,
        r: &str,
        ty: &Ty,
        method: &str,
        call: &mut syn::ExprMethodCall,
        span: Span,
    ) -> R<V> {
        let int = Ty::Int(IntK::I32);
        let (expected, js): (Vec<Ty>, String) = match (ty, method) {
            (Ty::Layout, "width" | "height") => {
                (vec![Ty::SizeMode], format!("rn.w({r}, \"{method}\", $0)"))
            }
            (Ty::Layout, "margin") => (vec![Ty::Insets], format!("rn.w({r}, \"margin\", $0)")),
            (Ty::Layout, "align_self") => {
                (vec![Ty::Alignment], format!("rn.w({r}, \"align_self\", $0)"))
            }
            (Ty::Layout, "constraints") => {
                (vec![Ty::Constraints], format!("rn.w({r}, \"constraints\", $0)"))
            }
            (Ty::Layout, "grid") => (vec![Ty::Placement], format!("rn.w({r}, \"grid\", $0)")),
            (Ty::Layout, "direction") => {
                (vec![Ty::Direction], format!("rn.w({r}, \"direction\", $0)"))
            }
            (Ty::Container, "padding") => (vec![Ty::Insets], format!("rn.w({r}, \"padding\", $0)")),
            (Ty::Container, "gap") => (vec![int], format!("rn.w({r}, \"gap\", $0)")),
            (Ty::Container, "align_items") => {
                (vec![Ty::Alignment], format!("rn.w({r}, \"align_items\", $0)"))
            }
            (Ty::Container, "overflow") => {
                (vec![Ty::Overflow], format!("rn.w({r}, \"overflow\", $0)"))
            }
            (Ty::Grid, "rows") => {
                (vec![Ty::Vec(Box::new(Ty::Track))], format!("rn.w({r}, \"rows\", [...$0])"))
            }
            (Ty::Grid, "gap") => (vec![int], format!("rn.w({r}, \"gap\", $0)")),
            (Ty::Grid, "padding") => (vec![Ty::Insets], format!("rn.w({r}, \"padding\", $0)")),
            (Ty::Placement, "span") => (vec![USIZE, USIZE], format!("rn.span({r}, $0, $1)")),
            (
                Ty::Constraints,
                "with_min_width" | "with_max_width" | "with_min_height" | "with_max_height",
            ) => (vec![int], format!("rn.constrain({r}, \"{}\", $0)", &method[5..])),
            (Ty::VirtualList, "overscan") => (vec![USIZE], format!("rn.w({r}, \"overscan\", $0)")),
            (Ty::VirtualList, "axis") => (vec![Ty::Axis], format!("rn.w({r}, \"axis\", $0)")),
            (Ty::A11y, "name" | "description" | "automation_id" | "text_value") => {
                let field = match method {
                    "name" => "name",
                    "description" => "desc",
                    "automation_id" => "auto",
                    _ => "value",
                };
                let value =
                    if method == "text_value" { "{ text: String($0) }" } else { "String($0)" };
                (vec![Ty::Str], format!("rn.w({r}, \"{field}\", {value})"))
            }
            (
                Ty::A11y,
                "focusable" | "expanded" | "selected" | "read_only" | "required" | "busy",
            ) => {
                let field = if method == "read_only" { "readonly" } else { method };
                (vec![Ty::Bool], format!("rn.w({r}, \"{field}\", $0)"))
            }
            (Ty::A11y, "checked") => (vec![Ty::Checked], format!("rn.w({r}, \"checked\", $0)")),
            (Ty::A11y, "live") => (vec![Ty::Live], format!("rn.w({r}, \"live\", $0)")),
            (Ty::A11y, "position_in_set") => (
                vec![Ty::Int(IntK::U32), Ty::Int(IntK::U32)],
                format!("rn.w({r}, \"position\", [$0, $1])"),
            ),
            (Ty::A11y, "labelled_by") => {
                (vec![Ty::Str], format!("rn.w({r}, \"labelledby\", String($0))"))
            }
            (Ty::A11y, "described_by" | "controls") => {
                let field = if method == "controls" { "controls" } else { "describedby" };
                (vec![Ty::Str], format!("rn.push({r}, \"{field}\", String($0))"))
            }
            (Ty::A11y, "range") => {
                (vec![Ty::Float(true); 4], format!("rn.a11yRange({r}, $0, $1, $2)"))
            }
            (Ty::Visual, "foreground" | "background" | "border") => {
                let field = match method {
                    "foreground" => "fg",
                    "background" => "bg",
                    _ => "border",
                };
                (vec![Ty::Color], format!("rn.w({r}, \"{field}\", $0)"))
            }
            (Ty::Visual, "border_radius") => {
                (vec![Ty::Int(IntK::U16)], format!("rn.w({r}, \"radius\", $0)"))
            }
            (Ty::Visual, "padding") => (vec![Ty::Insets], format!("rn.w({r}, \"padding\", $0)")),
            (Ty::Visual, "typography") => {
                (vec![Ty::Typography], format!("rn.w({r}, \"font\", $0)"))
            }
            _ => {
                return self.fail(
                    span,
                    &format!("`.{method}()` on {ty}"),
                    "the subset does not know this builder method",
                );
            }
        };
        let (pre, args, _) = arguments(self, &mut call.args, &expected)?;
        all.extend(pre);
        let mut js = js;
        for (index, argument) in args.iter().enumerate().rev() {
            js = js.replace(&format!("${index}"), argument);
        }
        Ok(seq(all, js, ty.clone()))
    }

    // ------------------------------------------------------------ macros ----

    pub fn macro_expr(&mut self, expr: &mut syn::Expr, expect: Option<&Ty>) -> R<V> {
        let syn::Expr::Macro(mac) = expr else { return Err(()) };
        let span = mac.span();
        let name = last_segment(&mac.mac.path);
        match name.as_str() {
            "rsx" => {
                let markup = match rustnative_markup::parse_markup(mac.mac.tokens.clone()) {
                    Ok(markup) => markup,
                    Err(error) => {
                        self.error(error);
                        return Err(());
                    }
                };
                if markup.context.is_some() {
                    return self.fail(
                        span,
                        "a component element",
                        "a client component's view is built from built-in elements",
                    );
                }
                let lowered = match rustnative_markup::lower(&markup) {
                    Ok(lowered) => lowered,
                    Err(error) => {
                        self.error(error);
                        return Err(());
                    }
                };
                let mut lowered: syn::Expr = match syn::parse2(lowered) {
                    Ok(lowered) => lowered,
                    Err(error) => {
                        self.error(error);
                        return Err(());
                    }
                };
                let value = self.expr(&mut lowered, expect)?;
                // The Rust keeps the expansion, with its arithmetic checked.
                *expr = lowered;
                Ok(value)
            }
            "classes" | "styles" => {
                let Ok(literal) = syn::parse2::<syn::LitStr>(mac.mac.tokens.clone()) else {
                    return self.fail(
                        span,
                        "a computed class string",
                        "a class string is a literal",
                    );
                };
                match crate::style::compile(&literal.value(), name == "styles") {
                    Ok(compiled) => {
                        if let Some(rule) = compiled.rule {
                            self.css.push(rule);
                        }
                        self.tokens.extend(compiled.tokens);
                        Ok(V::new(compiled.descriptor, Ty::Decl))
                    }
                    Err(message) => {
                        self.error(syn::Error::new(literal.span(), message));
                        Err(())
                    }
                }
            }
            "format" => {
                let (value, tokens) = self.format_args(mac.mac.tokens.clone(), span)?;
                mac.mac.tokens = tokens;
                Ok(value)
            }
            "println" | "print" | "eprintln" | "eprint" => {
                let (value, tokens) = self.format_args(mac.mac.tokens.clone(), span)?;
                mac.mac.tokens = tokens;
                let mut pre = value.pre;
                let log = if name.starts_with('e') { "console.error" } else { "console.log" };
                pre.push(format!("{log}({});", value.js));
                Ok(seq(pre, "undefined", Ty::Unit))
            }
            "panic" | "unreachable" | "todo" | "unimplemented" => {
                let (value, tokens) = if mac.mac.tokens.is_empty() {
                    (
                        V::new(
                            string(if name == "unreachable" {
                                "internal error: entered unreachable code"
                            } else {
                                "explicit panic"
                            }),
                            Ty::Str,
                        ),
                        TokenStream::new(),
                    )
                } else {
                    self.format_args(mac.mac.tokens.clone(), span)?
                };
                mac.mac.tokens = tokens;
                let mut pre = value.pre;
                pre.push(format!("rn.panic({});", value.js));
                Ok(seq(pre, "undefined", expect.cloned().unwrap_or(Ty::Unit)))
            }
            "assert" | "debug_assert" | "assert_eq" | "debug_assert_eq" | "assert_ne"
            | "debug_assert_ne" => {
                let parser =
                    syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
                let mut args = match syn::parse::Parser::parse2(parser, mac.mac.tokens.clone()) {
                    Ok(args) => args.into_iter().collect::<Vec<_>>(),
                    Err(error) => {
                        self.error(error);
                        return Err(());
                    }
                };
                let pairwise = name.contains("_eq") || name.contains("_ne");
                let count = if pairwise { 2 } else { 1 };
                if args.len() < count {
                    return Err(());
                }
                let mut pre = Vec::new();
                let mut values = Vec::new();
                for arg in args.iter_mut().take(count) {
                    let value = self.expr(arg, None)?;
                    pre.extend(value.pre.clone());
                    values.push(value);
                }
                let test = if pairwise {
                    let equal = if values[0].ty.is_primitive() {
                        format!("{} === {}", values[0].js, values[1].js)
                    } else {
                        format!("rn.eq({}, {})", values[0].js, values[1].js)
                    };
                    if name.contains("_ne") { format!("!({equal})") } else { equal }
                } else {
                    values[0].js.clone()
                };
                let message = if args.len() > count {
                    let rest: TokenStream = {
                        let rest = &args[count..];
                        quote!(#(#rest),*)
                    };
                    let (message, tokens) = self.format_args(rest, span)?;
                    pre.extend(message.pre);
                    let rewritten = &args[..count];
                    mac.mac.tokens = quote!(#(#rewritten),*, #tokens);
                    message.js
                } else {
                    let rewritten = &args;
                    mac.mac.tokens = quote!(#(#rewritten),*);
                    string(&format!("assertion failed: {}", quote!(#(#args),*)))
                };
                pre.push(format!("if (!({test})) rn.panic({message});"));
                Ok(seq(pre, "undefined", Ty::Unit))
            }
            "vec" => {
                let tokens = mac.mac.tokens.clone();
                // `vec![x; n]`
                if let Ok(repeat) = syn::parse2::<syn::ExprRepeat>(quote!([#tokens])) {
                    let mut repeat = syn::Expr::Repeat(repeat);
                    let value = self.expr(&mut repeat, expect)?;
                    if let syn::Expr::Repeat(repeat) = repeat {
                        let (value_expr, len) = (repeat.expr, repeat.len);
                        mac.mac.tokens = quote!(#value_expr; #len);
                    }
                    return Ok(value);
                }
                let mut array: syn::Expr = match syn::parse2(quote!([#tokens])) {
                    Ok(array) => array,
                    Err(error) => {
                        self.error(error);
                        return Err(());
                    }
                };
                let value = self.expr(&mut array, expect)?;
                if let syn::Expr::Array(array) = array {
                    let elements = array.elems;
                    mac.mac.tokens = quote!(#elements);
                }
                Ok(value)
            }
            "matches" => {
                let parsed = syn::parse::Parser::parse2(
                    |input: syn::parse::ParseStream<'_>| {
                        let expr: syn::Expr = input.parse()?;
                        input.parse::<syn::Token![,]>()?;
                        let pat = syn::Pat::parse_multi_with_leading_vert(input)?;
                        let guard = if input.peek(syn::Token![if]) {
                            input.parse::<syn::Token![if]>()?;
                            Some(input.parse::<syn::Expr>()?)
                        } else {
                            None
                        };
                        let _ = input.parse::<Option<syn::Token![,]>>();
                        Ok((expr, pat, guard))
                    },
                    mac.mac.tokens.clone(),
                );
                let (mut scrutinee, pat, mut guard) = match parsed {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        self.error(error);
                        return Err(());
                    }
                };
                let value = self.expr(&mut scrutinee, None)?;
                let mut pre = value.pre;
                let target = self.temp();
                pre.push(format!("const {target} = {};", value.js));
                self.push_scope();
                let (test, bindings) = self.pattern(&pat, &target, &value.ty)?;
                let result = self.temp();
                pre.push(format!("let {result} = false;"));
                let test = test.unwrap_or_else(|| "true".into());
                let mut inner = String::new();
                for (name, value, ty) in bindings {
                    let variable = self.bind(&name, ty);
                    let _ = writeln!(inner, "const {variable} = {value};");
                }
                match &mut guard {
                    Some(guard) => {
                        let guard_value = self.expr(guard, Some(&Ty::Bool))?;
                        inner.push_str(&guard_value.pre.join("\n"));
                        let _ = write!(inner, "\n{result} = {};", guard_value.js);
                    }
                    None => {
                        let _ = write!(inner, "{result} = true;");
                    }
                }
                self.pop_scope();
                pre.push(format!("if ({test}) {{\n{inner}\n}}"));
                mac.mac.tokens = match &guard {
                    Some(guard) => quote!(#scrutinee, #pat if #guard),
                    None => quote!(#scrutinee, #pat),
                };
                Ok(seq(pre, result, Ty::Bool))
            }
            other => self.fail(span, &format!("`{other}!`"), "the subset does not know this macro"),
        }
    }

    /// A format string and its arguments: the JavaScript string, and the
    /// arguments' Rust rewritten.
    pub fn format_args(&mut self, tokens: TokenStream, span: Span) -> R<(V, TokenStream)> {
        let parsed = syn::parse::Parser::parse2(
            |input: syn::parse::ParseStream<'_>| {
                let literal: syn::LitStr = input.parse()?;
                let mut args: Vec<(Option<syn::Ident>, syn::Expr)> = Vec::new();
                while input.parse::<Option<syn::Token![,]>>()?.is_some() {
                    if input.is_empty() {
                        break;
                    }
                    if input.peek(syn::Ident)
                        && input.peek2(syn::Token![=])
                        && !input.peek2(syn::Token![==])
                    {
                        let name: syn::Ident = input.parse()?;
                        input.parse::<syn::Token![=]>()?;
                        args.push((Some(name), input.parse()?));
                    } else {
                        args.push((None, input.parse()?));
                    }
                }
                Ok((literal, args))
            },
            tokens,
        );
        let (literal, mut args) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                self.error(error);
                return Err(());
            }
        };
        let pieces = match parse_format(&literal.value()) {
            Ok(pieces) => pieces,
            Err(message) => return self.fail(literal.span(), "this format string", &message),
        };
        let mut pre = Vec::new();
        let mut values: Vec<V> = Vec::new();
        for (_, arg) in &mut args {
            let value = self.expr(arg, None)?;
            pre.extend(value.pre.clone());
            values.push(value);
        }
        let mut parts = Vec::new();
        let mut next = 0;
        for piece in pieces {
            match piece {
                Piece::Text(text) => parts.push(string(&text)),
                Piece::Arg { selector, spec } => {
                    let value = match &selector {
                        Selector::Next => {
                            let value = values.get(next).cloned();
                            next += 1;
                            value
                        }
                        Selector::Index(index) => values.get(*index).cloned(),
                        Selector::Name(name) => {
                            let position = args.iter().position(|(named, _)| {
                                named.as_ref().is_some_and(|named| named == name)
                            });
                            match position {
                                Some(position) => values.get(position).cloned(),
                                None => match self.lookup(name) {
                                    Some(binding) => {
                                        Some(V::new(binding.js.clone(), binding.ty.clone()))
                                    }
                                    None => {
                                        return self.fail(
                                            literal.span(),
                                            &format!("`{{{name}}}`"),
                                            "no such variable",
                                        );
                                    }
                                },
                            }
                        }
                    };
                    let Some(value) = value else {
                        return self.fail(literal.span(), "this placeholder", "no argument for it");
                    };
                    parts.push(self.format_one(&value, &spec, literal.span())?);
                }
            }
        }
        let js = if parts.is_empty() { "\"\"".to_owned() } else { parts.join(" + ") };
        let rewritten = args.iter().map(|(name, expr)| match name {
            Some(name) => quote!(#name = #expr),
            None => quote!(#expr),
        });
        let _ = span;
        Ok((seq(pre, format!("({js})"), Ty::Str), quote!(#literal #(, #rewritten)*)))
    }

    fn format_one(&mut self, value: &V, spec: &Spec, span: Span) -> R<String> {
        let js = &value.js;
        let numeric = value.ty.is_numeric();
        let text = match (spec.kind.as_str(), &value.ty) {
            ("", Ty::Float(f32)) => match spec.precision {
                Some(precision) => format!("rn.fixed({js}, {precision})"),
                None => format!("rn.float({js}, {})", if *f32 { "\"f32\"" } else { "\"f64\"" }),
            },
            ("", Ty::Int(_)) => format!("String({js})"),
            ("", Ty::Str | Ty::Char | Ty::NodeId) => match spec.precision {
                Some(precision) => format!("[...{js}].slice(0, {precision}).join(\"\")"),
                None => js.clone(),
            },
            ("", Ty::Bool) => format!("String({js})"),
            ("", Ty::ServerFnError | Ty::ParseError) => format!("rn.errorText({js})"),
            ("", Ty::Date) => format!("rn.dateText({js})"),
            ("", Ty::Opaque(_)) => format!("rn.display({js}, \"any\")"),
            ("?", Ty::Str | Ty::NodeId) => format!("rn.debugString({js}, '\"')"),
            ("?", Ty::Char) => format!("rn.debugString({js}, \"'\")"),
            ("?", Ty::Int(_) | Ty::Bool) => format!("String({js})"),
            ("x" | "X" | "b" | "o", Ty::Int(_)) => js.clone(),
            (kind, ty) => {
                return self.fail(
                    span,
                    &format!("`{{:{kind}}}` of {ty}"),
                    "the subset formats numbers, strings, characters, and booleans",
                );
            }
        };
        let radix = match spec.kind.as_str() {
            "x" | "X" => 16,
            "b" => 2,
            "o" => 8,
            _ => 0,
        };
        let plain =
            spec.width.is_none() && spec.fill.is_none() && !spec.sign && !spec.zero && radix == 0;
        if plain {
            return Ok(text);
        }
        let bits = match &value.ty {
            Ty::Int(kind) => match self.resolve(*kind).name() {
                "i8" | "u8" => 8,
                "i16" | "u16" => 16,
                "i32" | "u32" => 32,
                _ => 64,
            },
            _ => 64,
        };
        let options = format!(
            "{{ fill: {}, align: {}, sign: {}, width: {}, zero: {}, radix: {radix}, upper: {}, alternate: {}, bits: {bits} }}",
            spec.fill.map_or("null".into(), |fill| string(&fill.to_string())),
            spec.align.map_or("null".into(), |align| string(&align.to_string())),
            spec.sign,
            spec.width.unwrap_or(0),
            spec.zero,
            spec.kind == "X",
            spec.alternate,
        );
        Ok(format!("rn.pad({text}, {options}, {numeric})"))
    }
}

fn strip_ref(expr: &mut syn::Expr) -> &mut syn::Expr {
    match expr {
        syn::Expr::Reference(reference) => strip_ref(&mut reference.expr),
        other => other,
    }
}

/// An effect beyond the built-in ones: the browser capabilities (Web
/// milestone E) — its runtime name.
pub fn capability_effect(method: &str) -> Option<&'static str> {
    Some(match method {
        "share" => "share",
        "locate" => "locate",
        "permission" => "permission",
        "request_permission" => "requestPermission",
        "open_file" => "openFile",
        "save_file" => "saveFile",
        "db_put" => "dbPut",
        "db_get" => "dbGet",
        "cache_put" => "cachePut",
        "cache_get" => "cacheGet",
        "socket_open" => "socketOpen",
        "socket_send" => "socketSend",
        "socket_close" => "socketClose",
        "worker" => "worker",
        "vibrate" => "vibrate",
        "online" => "online",
        "visibility" => "visibility",
        "media" => "media",
        "bluetooth" => "bluetooth",
        "sensor" => "sensor",
        _ => return None,
    })
}

// ------------------------------------------------------ format strings ----

#[derive(Debug, Clone, Default)]
pub struct Spec {
    pub fill: Option<char>,
    pub align: Option<char>,
    pub sign: bool,
    pub alternate: bool,
    pub zero: bool,
    pub width: Option<usize>,
    pub precision: Option<usize>,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub enum Selector {
    Next,
    Index(usize),
    Name(String),
}

#[derive(Debug, Clone)]
pub enum Piece {
    Text(String),
    Arg { selector: Selector, spec: Spec },
}

/// Parses a format string (`std::fmt`'s grammar, without `*` and `name$`
/// widths).
pub fn parse_format(text: &str) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '{' => {
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(other) => inner.push(other),
                        None => return Err("an unclosed `{`".into()),
                    }
                }
                if !literal.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut literal)));
                }
                let (argument, spec) = inner.split_once(':').unwrap_or((inner.as_str(), ""));
                let argument = argument.trim();
                let selector = if argument.is_empty() {
                    Selector::Next
                } else if let Ok(index) = argument.parse() {
                    Selector::Index(index)
                } else {
                    Selector::Name(argument.to_owned())
                };
                pieces.push(Piece::Arg { selector, spec: parse_spec(spec)? });
            }
            '}' => return Err("an unmatched `}`".into()),
            other => literal.push(other),
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Text(literal));
    }
    Ok(pieces)
}

fn parse_spec(text: &str) -> Result<Spec, String> {
    let mut spec = Spec::default();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    let is_align = |ch: char| matches!(ch, '<' | '>' | '^');
    if chars.len() >= 2 && is_align(chars[1]) {
        spec.fill = Some(chars[0]);
        spec.align = Some(chars[1]);
        index = 2;
    } else if !chars.is_empty() && is_align(chars[0]) {
        spec.align = Some(chars[0]);
        index = 1;
    }
    if chars.get(index) == Some(&'+') {
        spec.sign = true;
        index += 1;
    } else if chars.get(index) == Some(&'-') {
        index += 1;
    }
    if chars.get(index) == Some(&'#') {
        spec.alternate = true;
        index += 1;
    }
    if chars.get(index) == Some(&'0') {
        spec.zero = true;
        index += 1;
    }
    let digits: String = chars[index..].iter().take_while(|ch| ch.is_ascii_digit()).collect();
    if !digits.is_empty() {
        spec.width = digits.parse().ok();
        index += digits.len();
    }
    if chars.get(index) == Some(&'.') {
        index += 1;
        let digits: String = chars[index..].iter().take_while(|ch| ch.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err("a precision needs digits (`.*` and `.name$` are not in the subset)".into());
        }
        spec.precision = digits.parse().ok();
        index += digits.len();
    }
    let kind: String = chars[index..].iter().collect();
    if chars.get(index) == Some(&'$') || kind.contains('$') {
        return Err("`name$` widths are not in the subset".into());
    }
    if !matches!(kind.as_str(), "" | "?" | "x" | "X" | "b" | "o") {
        return Err(format!("the `{kind}` format is not in the subset"));
    }
    spec.kind = kind;
    Ok(spec)
}

// --------------------------------------------------- framework structs ----

/// A struct literal of a framework type (or, for data from outside the
/// module, an object with the fields as written).
pub fn framework_struct(
    cx: &mut Cx<'_>,
    literal: &mut syn::ExprStruct,
    owner: &str,
    name: &str,
    expect: Option<&Ty>,
) -> R<V> {
    let span = literal.span();
    let (fields, ty, wrap): (Vec<(String, Ty)>, Ty, Option<String>) = match (owner, name) {
        (_, "EdgeInsets" | "Typography" | "CalendarDate" | "KeyModifiers") => {
            let ty = match name {
                "EdgeInsets" => Ty::Insets,
                "Typography" => Ty::Typography,
                "CalendarDate" => Ty::Date,
                _ => Ty::Modifiers,
            };
            (framework_struct_fields(name).unwrap_or_default(), ty, None)
        }
        (_, "GridPlacement") => (
            vec![
                ("row".into(), USIZE),
                ("column".into(), USIZE),
                ("row_span".into(), USIZE),
                ("column_span".into(), USIZE),
            ],
            Ty::Placement,
            None,
        ),
        ("Control", variant) => {
            let fields: Vec<(String, Ty)> = match variant {
                "Checkbox" => vec![("label".into(), Ty::Str), ("checked".into(), Ty::Bool)],
                "Radio" => vec![("label".into(), Ty::Str), ("selected".into(), Ty::Bool)],
                "Toggle" => vec![("label".into(), Ty::Str), ("on".into(), Ty::Bool)],
                "Slider" | "Spinner" => vec![
                    ("value".into(), Ty::Int(IntK::I64)),
                    ("min".into(), Ty::Int(IntK::I64)),
                    ("max".into(), Ty::Int(IntK::I64)),
                ],
                "Progress" => vec![("percent".into(), Ty::Opt(Box::new(Ty::Int(IntK::U8))))],
                "Select" => vec![
                    ("options".into(), Ty::Vec(Box::new(Ty::Str))),
                    ("selected".into(), Ty::Opt(Box::new(USIZE))),
                ],
                "ListBox" => vec![
                    ("items".into(), Ty::Vec(Box::new(Ty::Str))),
                    ("selected".into(), Ty::Opt(Box::new(USIZE))),
                ],
                "DatePicker" => vec![("date".into(), Ty::Date)],
                "Link" => vec![("text".into(), Ty::Str)],
                "MultilineText" => vec![("value".into(), Ty::Str)],
                other => {
                    return cx.fail(
                        span,
                        &format!("`Control::{other}`"),
                        "images are not in the subset",
                    );
                }
            };
            (fields, Ty::Control, Some(variant.to_owned()))
        }
        ("AccessibilityRole", "Heading") => {
            (vec![("level".into(), Ty::Int(IntK::U8))], Ty::Role, Some("Heading".into()))
        }
        _ => {
            // Data from outside the module, written field by field: an
            // object with the fields as named (serde's default shape).
            let mut pre = Vec::new();
            let mut fields = Vec::new();
            for field in &mut literal.fields {
                let syn::Member::Named(member) = &field.member else {
                    return cx.fail(field.span(), "a numbered field", "name the field");
                };
                let member = member.to_string();
                let value = cx.expr(&mut field.expr, None)?;
                let value = cx.copy_if_place(&field.expr, value);
                pre.extend(value.pre);
                fields.push(format!("{}: {}", string(&member), value.js));
            }
            if literal.rest.is_some() {
                return cx.fail(
                    span,
                    "`..` on a type from outside the module",
                    "write every field",
                );
            }
            let _ = expect;
            return Ok(seq(
                pre,
                format!("{{ {} }}", fields.join(", ")),
                Ty::Opaque(name.to_owned()),
            ));
        }
    };
    let mut pre = Vec::new();
    let mut values = Vec::new();
    for field in &mut literal.fields {
        let syn::Member::Named(member) = &field.member else {
            return cx.fail(field.span(), "a numbered field", "name the field");
        };
        let member = member.to_string();
        let Some((_, field_ty)) = fields.iter().find(|(name, _)| *name == member) else {
            return cx.fail(
                field.span(),
                &format!("`{member}`"),
                &format!("`{name}` has no such field"),
            );
        };
        let value = cx.expr(&mut field.expr, Some(field_ty))?;
        pre.extend(value.pre);
        let js = if *field_ty == Ty::Str {
            format!("String({})", value.js)
        } else if matches!(field_ty, Ty::Vec(_)) {
            format!("[...{}].map((x) => x)", value.js)
        } else {
            value.js
        };
        values.push(format!("{member}: {js}"));
    }
    let object = if let Some(rest) = &mut literal.rest {
        let base = if crate::expr::is_default_call(rest) {
            match cx.module.default_js(&ty, span) {
                Ok(js) => js,
                Err(error) => {
                    cx.error(error);
                    return Err(());
                }
            }
        } else {
            let value = cx.expr(rest, Some(&ty))?;
            pre.extend(value.pre);
            format!("rn.clone({})", value.js)
        };
        format!("{{ ...{base}, {} }}", values.join(", "))
    } else {
        format!("{{ {} }}", values.join(", "))
    };
    let js = match wrap {
        Some(_) if ty == Ty::Role => {
            let level: String = values
                .first()
                .and_then(|value| value.split_once(": "))
                .map_or("1".into(), |(_, level)| level.to_owned());
            format!("{{ Heading: {level} }}")
        }
        Some(variant) => format!("{{ {}: {object} }}", string(&variant)),
        None => object,
    };
    Ok(seq(pre, js, ty))
}
