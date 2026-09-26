//! The element table: one element per `Node` constructor, one attribute
//! per builder method.
//!
//! This table is the whole of the grammar's vocabulary. It is what the
//! lowering reads, what diagnostics suggest from, what the editor proxy
//! completes from, and what `rustnative describe` publishes — so an element
//! or attribute cannot exist in one of those and not the others. A new node
//! kind or modifier is added here in the same commit that adds the builder
//! form (CONTRIBUTING).

/// Which `Node` constructor an element lowers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constructor {
    /// `Node::label_with_layout(key, text, layout)`.
    Label,
    /// `Node::button_with_layout(key, text, layout)`.
    Button,
    /// `Node::text_input_with_layout(key, value, layout)`.
    TextInput,
    /// `Node::canvas(key, draw_list, layout)`.
    Canvas,
    /// `Node::native_surface(key, layout)`.
    Surface,
    /// `Node::foreign(key, kind, layout)`.
    Foreign,
    /// `Node::tab_bar(key, labels, selected, layout)`.
    TabBar,
    /// `Node::column_with_layout(key, children, layout, style)`.
    Column,
    /// `Node::row_with_layout(key, children, layout, style)`.
    Row,
    /// `Node::virtual_list_with_layout(key, list, layout, children)`.
    VirtualList,
    /// `Node::grid(key, tracks, layout, children)`.
    Grid,
    /// `Node::control_with_layout(key, Control::Checkbox{…}, layout)`.
    Checkbox,
    /// `Node::control_with_layout(key, Control::Radio{…}, layout)`.
    Radio,
    /// `Node::control_with_layout(key, Control::Toggle{…}, layout)`.
    Toggle,
    /// `Node::control_with_layout(key, Control::Slider{…}, layout)`.
    Slider,
    /// `Node::control_with_layout(key, Control::Progress{…}, layout)`.
    Progress,
    /// `Node::control_with_layout(key, Control::Select{…}, layout)`.
    Select,
    /// `Node::control_with_layout(key, Control::ListBox{…}, layout)`.
    ListBox,
    /// `Node::control_with_layout(key, Control::DatePicker{…}, layout)`.
    DatePicker,
    /// `Node::control_with_layout(key, Control::Spinner{…}, layout)`.
    Spinner,
    /// `Node::control_with_layout(key, Control::Separator{…}, layout)`.
    Separator,
    /// `Node::control_with_layout(key, Control::Link{…}, layout)`.
    Link,
    /// `Node::control_with_layout(key, Control::MultilineText{…}, layout)`.
    MultilineText,
    /// `Node::control_with_layout(key, Control::Image{…}, layout)`.
    Image,
}

/// What kind of attribute an attribute is, which decides how it lowers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrKind {
    /// The constructor's key.
    Key,
    /// A constructor argument other than the key.
    Argument,
    /// A `LayoutStyle` field, set through its builder method.
    Layout,
    /// A `ColumnStyle`/`RowStyle` field.
    Container,
    /// A `with_*` modifier (or `disabled`/`hidden`) applied to the node.
    Modifier,
}

/// One attribute an element accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttrSpec {
    /// The attribute's name.
    pub name: &'static str,
    /// How it lowers.
    pub kind: AttrKind,
    /// The builder method it calls, as documentation names it.
    pub method: &'static str,
    /// Whether the element cannot be written without it.
    pub required: bool,
    /// Whether it may be written with no value (`disabled`).
    pub flag: bool,
}

/// One element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementSpec {
    /// The element's name.
    pub name: &'static str,
    /// The constructor it lowers to.
    pub constructor: Constructor,
    /// Every attribute it accepts.
    pub attrs: Vec<AttrSpec>,
    /// Whether it takes children.
    pub children: bool,
}

const fn attr(name: &'static str, kind: AttrKind, method: &'static str) -> AttrSpec {
    AttrSpec { name, kind, method, required: false, flag: false }
}

const fn required(name: &'static str, kind: AttrKind, method: &'static str) -> AttrSpec {
    AttrSpec { name, kind, method, required: true, flag: false }
}

const fn flag(name: &'static str, method: &'static str) -> AttrSpec {
    AttrSpec { name, kind: AttrKind::Modifier, method, required: false, flag: true }
}

/// The attributes every element accepts: its key, its `LayoutStyle`, and
/// every `with_*` modifier.
fn universal() -> Vec<AttrSpec> {
    vec![
        required("key", AttrKind::Key, "the constructor's `key`"),
        attr("width", AttrKind::Layout, "LayoutStyle::width"),
        attr("height", AttrKind::Layout, "LayoutStyle::height"),
        attr("margin", AttrKind::Layout, "LayoutStyle::margin"),
        attr("align_self", AttrKind::Layout, "LayoutStyle::align_self"),
        attr("constraints", AttrKind::Layout, "LayoutStyle::constraints"),
        attr("direction", AttrKind::Layout, "LayoutStyle::direction"),
        attr("grid", AttrKind::Layout, "LayoutStyle::grid"),
        attr("accessibility", AttrKind::Modifier, "Node::with_accessibility"),
        attr("class", AttrKind::Modifier, "Node::with_class(classes!(\"…\"))"),
        attr(
            "style",
            AttrKind::Modifier,
            "Node::with_style, or Node::with_declarations(styles!(\"…\")) for a string",
        ),
        attr("input", AttrKind::Modifier, "Node::with_input"),
        attr("opacity", AttrKind::Modifier, "Node::with_opacity"),
        attr("transition", AttrKind::Modifier, "Node::with_transition"),
        attr("item_index", AttrKind::Modifier, "Node::with_item_index"),
        attr("command", AttrKind::Modifier, "Node::with_command"),
        attr("cursor", AttrKind::Modifier, "Node::with_cursor"),
        attr("shared_id", AttrKind::Modifier, "Node::with_shared_id"),
        flag("disabled", "Node::disabled"),
        flag("hidden", "Node::hidden"),
    ]
}

fn container(style: &'static str) -> Vec<AttrSpec> {
    let method = |field: &'static str| -> &'static str {
        match (style, field) {
            ("ColumnStyle", "padding") => "ColumnStyle::padding",
            ("ColumnStyle", "gap") => "ColumnStyle::gap",
            ("ColumnStyle", "align_items") => "ColumnStyle::align_items",
            ("ColumnStyle", "overflow") => "ColumnStyle::overflow",
            (_, "padding") => "RowStyle::padding",
            (_, "gap") => "RowStyle::gap",
            (_, "align_items") => "RowStyle::align_items",
            _ => "RowStyle::overflow",
        }
    };
    ["padding", "gap", "align_items", "overflow"]
        .into_iter()
        .map(|field| attr(field, AttrKind::Container, method(field)))
        .collect()
}

fn element(
    name: &'static str,
    constructor: Constructor,
    own: Vec<AttrSpec>,
    children: bool,
) -> ElementSpec {
    let mut attrs = universal();
    attrs.extend(own);
    ElementSpec { name, constructor, attrs, children }
}

/// Every built-in element.
#[must_use]
pub fn element_table() -> Vec<ElementSpec> {
    vec![
        element(
            "Label",
            Constructor::Label,
            vec![required("text", AttrKind::Argument, "Node::label_with_layout")],
            false,
        ),
        element(
            "Button",
            Constructor::Button,
            vec![required("text", AttrKind::Argument, "Node::button_with_layout")],
            false,
        ),
        element(
            "TextInput",
            Constructor::TextInput,
            vec![required("value", AttrKind::Argument, "Node::text_input_with_layout")],
            false,
        ),
        element(
            "Canvas",
            Constructor::Canvas,
            vec![required("draw_list", AttrKind::Argument, "Node::canvas")],
            false,
        ),
        element("Surface", Constructor::Surface, vec![], false),
        element(
            "Foreign",
            Constructor::Foreign,
            vec![required("kind", AttrKind::Argument, "Node::foreign")],
            false,
        ),
        element(
            "TabBar",
            Constructor::TabBar,
            vec![
                required("labels", AttrKind::Argument, "Node::tab_bar"),
                required("selected", AttrKind::Argument, "Node::tab_bar"),
            ],
            false,
        ),
        element("Column", Constructor::Column, container("ColumnStyle"), true),
        element("Row", Constructor::Row, container("RowStyle"), true),
        element(
            "VirtualList",
            Constructor::VirtualList,
            vec![required("list", AttrKind::Argument, "Node::virtual_list_with_layout")],
            true,
        ),
        element(
            "Grid",
            Constructor::Grid,
            vec![required("tracks", AttrKind::Argument, "Node::grid")],
            true,
        ),
        element(
            "Checkbox",
            Constructor::Checkbox,
            vec![
                required("label", AttrKind::Argument, "Control::Checkbox"),
                required("checked", AttrKind::Argument, "Control::Checkbox"),
            ],
            false,
        ),
        element(
            "Radio",
            Constructor::Radio,
            vec![
                required("label", AttrKind::Argument, "Control::Radio"),
                required("selected", AttrKind::Argument, "Control::Radio"),
            ],
            false,
        ),
        element(
            "Toggle",
            Constructor::Toggle,
            vec![
                required("label", AttrKind::Argument, "Control::Toggle"),
                required("on", AttrKind::Argument, "Control::Toggle"),
            ],
            false,
        ),
        element(
            "Slider",
            Constructor::Slider,
            vec![
                required("value", AttrKind::Argument, "Control::Slider"),
                required("min", AttrKind::Argument, "Control::Slider"),
                required("max", AttrKind::Argument, "Control::Slider"),
            ],
            false,
        ),
        element(
            "Progress",
            Constructor::Progress,
            vec![required("percent", AttrKind::Argument, "Control::Progress")],
            false,
        ),
        element(
            "Select",
            Constructor::Select,
            vec![
                required("options", AttrKind::Argument, "Control::Select"),
                required("selected", AttrKind::Argument, "Control::Select"),
            ],
            false,
        ),
        element(
            "ListBox",
            Constructor::ListBox,
            vec![
                required("items", AttrKind::Argument, "Control::ListBox"),
                required("selected", AttrKind::Argument, "Control::ListBox"),
            ],
            false,
        ),
        element(
            "DatePicker",
            Constructor::DatePicker,
            vec![required("date", AttrKind::Argument, "Control::DatePicker")],
            false,
        ),
        element(
            "Spinner",
            Constructor::Spinner,
            vec![
                required("value", AttrKind::Argument, "Control::Spinner"),
                required("min", AttrKind::Argument, "Control::Spinner"),
                required("max", AttrKind::Argument, "Control::Spinner"),
            ],
            false,
        ),
        element("Separator", Constructor::Separator, vec![], false),
        element(
            "Link",
            Constructor::Link,
            vec![required("text", AttrKind::Argument, "Control::Link")],
            false,
        ),
        element(
            "MultilineText",
            Constructor::MultilineText,
            vec![required("value", AttrKind::Argument, "Control::MultilineText")],
            false,
        ),
        element(
            "Image",
            Constructor::Image,
            vec![required("image", AttrKind::Argument, "Control::Image")],
            false,
        ),
    ]
}

/// The built-in element named `name`.
#[must_use]
pub fn element_spec(name: &str) -> Option<ElementSpec> {
    element_table().into_iter().find(|spec| spec.name == name)
}

/// Whether `name` is a built-in element (anything else is a component).
#[must_use]
pub fn is_builtin(name: &str) -> bool {
    element_spec(name).is_some()
}

/// The attribute name closest to `wanted` among `candidates`, if close
/// enough to be a plausible typo.
#[must_use]
pub fn nearest<'a>(wanted: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    candidates
        .into_iter()
        .map(|candidate| (edit_distance(wanted, candidate), candidate))
        .filter(|(distance, candidate)| *distance <= (candidate.len().max(wanted.len()) / 3).max(2))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, left) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, right) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(left != right);
            current.push(substitution.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_element_has_a_key_and_unique_attributes() {
        for spec in element_table() {
            assert!(
                spec.attrs.iter().any(|attr| attr.name == "key" && attr.required),
                "{}",
                spec.name
            );
            let mut names: Vec<_> = spec.attrs.iter().map(|attr| attr.name).collect();
            names.sort_unstable();
            let before = names.len();
            names.dedup();
            assert_eq!(before, names.len(), "{} has a duplicate attribute", spec.name);
        }
    }

    #[test]
    fn typos_find_their_attribute() {
        let spec = element_spec("Column").expect("Column");
        let names = spec.attrs.iter().map(|attr| attr.name);
        assert_eq!(nearest("paddng", names), Some("padding"));
        assert_eq!(nearest("zzzzzz", ["padding", "gap"]), None);
    }
}
