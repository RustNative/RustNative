//! The types the client subset knows, and how a Rust type is read into one.
//!
//! The translator needs static types where the two languages differ: an
//! integer's width decides its overflow check, `/` means truncating
//! division for integers and not for floats, `+` concatenates strings, and
//! `==` compares structure for data and identity for primitives. Anything
//! whose type the translator cannot see is [`Ty::Opaque`], and an operation
//! whose meaning depends on a type it cannot see is refused.

use std::fmt;

/// An integer's kind; `Var` is a literal whose kind later use decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntK {
    I8,
    I16,
    I32,
    I64,
    Isize,
    U8,
    U16,
    U32,
    U64,
    Usize,
    Var(u32),
}

impl IntK {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "isize" => Self::Isize,
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "usize" => Self::Usize,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 | Self::Var(_) => "i32",
            Self::I64 => "i64",
            Self::Isize => "isize",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::Usize => "usize",
        }
    }
}

/// A type in the client subset.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Ty {
    Int(IntK),
    /// `true` for `f32`.
    Float(bool),
    Bool,
    Char,
    Str,
    #[default]
    Unit,
    Vec(Box<Ty>),
    Opt(Box<Ty>),
    Res(Box<Ty>, Box<Ty>),
    Tuple(Vec<Ty>),
    /// A struct or enum defined in the client module.
    Adt(String),
    /// A value computed eagerly as a JavaScript array: an iterator.
    Iter(Box<Ty>),
    /// A function value (a closure, a variant constructor), by its result.
    Fn(Box<Ty>),
    // Framework types.
    Node,
    NodeId,
    Event,
    Effects(Box<Ty>),
    KeyCode,
    Modifiers,
    Date,
    Lifecycle,
    Layout,
    SizeMode,
    Insets,
    Constraints,
    Alignment,
    Overflow,
    Direction,
    Placement,
    Container,
    Grid,
    Track,
    VirtualList,
    Extent,
    Axis,
    A11y,
    Role,
    Checked,
    Live,
    Visual,
    Color,
    Typography,
    ControlState,
    Cursor,
    Control,
    Decl,
    ServerFnError,
    ParseError,
    /// A type the translator cannot see into.
    Opaque(String),
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(kind) => f.write_str(kind.name()),
            Self::Float(f32) => f.write_str(if *f32 { "f32" } else { "f64" }),
            Self::Bool => f.write_str("bool"),
            Self::Char => f.write_str("char"),
            Self::Str => f.write_str("String"),
            Self::Unit => f.write_str("()"),
            Self::Vec(inner) => write!(f, "Vec<{inner}>"),
            Self::Opt(inner) => write!(f, "Option<{inner}>"),
            Self::Res(ok, err) => write!(f, "Result<{ok}, {err}>"),
            Self::Tuple(items) => {
                let items: Vec<String> = items.iter().map(ToString::to_string).collect();
                write!(f, "({})", items.join(", "))
            }
            Self::Adt(name) | Self::Opaque(name) => f.write_str(name),
            Self::Iter(inner) => write!(f, "an iterator over {inner}"),
            Self::Fn(output) => write!(f, "a function returning {output}"),
            Self::Effects(message) => write!(f, "Effects<{message}>"),
            other => write!(f, "{other:?}"),
        }
    }
}

impl Ty {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int(_) | Self::Float(_))
    }

    /// Whether values of this type are JavaScript primitives, compared by
    /// `===` and copied by assignment.
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Self::Int(_)
                | Self::Float(_)
                | Self::Bool
                | Self::Char
                | Self::Str
                | Self::Unit
                | Self::NodeId
                | Self::Alignment
                | Self::Overflow
                | Self::Direction
                | Self::Axis
                | Self::Checked
                | Self::Live
                | Self::ControlState
                | Self::Cursor
                | Self::Lifecycle
        )
    }

    /// The element type of an iterable.
    pub fn element(&self) -> Option<Ty> {
        match self {
            Self::Vec(inner) | Self::Iter(inner) => Some((**inner).clone()),
            Self::Opt(inner) => Some((**inner).clone()),
            Self::Str => Some(Self::Char),
            _ => None,
        }
    }

    /// The display kind `rn.display` takes.
    pub fn display_kind(&self) -> &'static str {
        match self {
            Self::Float(true) => "f32",
            Self::Float(false) => "f64",
            Self::Int(_) => "int",
            Self::Str | Self::NodeId => "str",
            Self::Char => "char",
            Self::Bool => "bool",
            _ => "any",
        }
    }
}

/// The last segment of a path, as a string.
pub fn last_segment(path: &syn::Path) -> String {
    path.segments.last().map(|segment| segment.ident.to_string()).unwrap_or_default()
}

fn generic_args(segment: &syn::PathSegment) -> Vec<&syn::Type> {
    match &segment.arguments {
        syn::PathArguments::AngleBracketed(arguments) => arguments
            .args
            .iter()
            .filter_map(|argument| match argument {
                syn::GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Reads a Rust type, given the names of the module's own types.
pub fn read(ty: &syn::Type, local: &dyn Fn(&str) -> bool) -> Ty {
    match ty {
        syn::Type::Reference(reference) => read(&reference.elem, local),
        syn::Type::Paren(paren) => read(&paren.elem, local),
        syn::Type::Group(group) => read(&group.elem, local),
        syn::Type::Tuple(tuple) if tuple.elems.is_empty() => Ty::Unit,
        syn::Type::Tuple(tuple) => {
            Ty::Tuple(tuple.elems.iter().map(|ty| read(ty, local)).collect())
        }
        syn::Type::Slice(slice) => Ty::Vec(Box::new(read(&slice.elem, local))),
        syn::Type::Array(array) => Ty::Vec(Box::new(read(&array.elem, local))),
        syn::Type::Path(path) if path.qself.is_none() => {
            let Some(segment) = path.path.segments.last() else { return Ty::Opaque("?".into()) };
            let name = segment.ident.to_string();
            let args = generic_args(segment);
            let arg =
                |index: usize| args.get(index).map_or(Ty::Opaque("_".into()), |ty| read(ty, local));
            if let Some(kind) = IntK::from_name(&name) {
                return Ty::Int(kind);
            }
            match name.as_str() {
                "f64" => Ty::Float(false),
                "f32" => Ty::Float(true),
                "bool" => Ty::Bool,
                "char" => Ty::Char,
                "String" | "str" => Ty::Str,
                "Vec" | "VecDeque" => Ty::Vec(Box::new(arg(0))),
                "Option" => Ty::Opt(Box::new(arg(0))),
                "Result" => Ty::Res(Box::new(arg(0)), Box::new(arg(1))),
                "Box" | "Rc" | "Arc" | "Cow" => arg(0),
                "Node" => Ty::Node,
                "NodeId" => Ty::NodeId,
                "Event" => Ty::Event,
                "Effects" => Ty::Effects(Box::new(arg(0))),
                "KeyCode" => Ty::KeyCode,
                "KeyModifiers" => Ty::Modifiers,
                "CalendarDate" => Ty::Date,
                "Lifecycle" => Ty::Lifecycle,
                "LayoutStyle" => Ty::Layout,
                "SizeMode" => Ty::SizeMode,
                "EdgeInsets" => Ty::Insets,
                "Constraints" => Ty::Constraints,
                "Alignment" => Ty::Alignment,
                "Overflow" => Ty::Overflow,
                "LayoutDirection" => Ty::Direction,
                "GridPlacement" => Ty::Placement,
                "ColumnStyle" | "RowStyle" => Ty::Container,
                "GridStyle" => Ty::Grid,
                "Track" => Ty::Track,
                "VirtualListStyle" => Ty::VirtualList,
                "ItemExtent" => Ty::Extent,
                "Axis" => Ty::Axis,
                "AccessibilityInfo" => Ty::A11y,
                "AccessibilityRole" => Ty::Role,
                "CheckedState" => Ty::Checked,
                "LiveRegion" => Ty::Live,
                "VisualStyle" => Ty::Visual,
                "Color" => Ty::Color,
                "Typography" => Ty::Typography,
                "ControlState" => Ty::ControlState,
                "Cursor" => Ty::Cursor,
                "Control" => Ty::Control,
                "DeclarationSet" => Ty::Decl,
                "ServerFnError" => Ty::ServerFnError,
                "ParseIntError" | "ParseFloatError" | "ParseBoolError" => Ty::ParseError,
                "Self" => Ty::Opaque("Self".into()),
                other if local(other) => Ty::Adt(other.to_owned()),
                other => Ty::Opaque(other.to_owned()),
            }
        }
        _ => Ty::Opaque(quote::quote!(#ty).to_string()),
    }
}
