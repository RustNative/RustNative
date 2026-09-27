//! Native DOM realization (`PLAN.md` Web milestone B): each framework node
//! becomes the semantic element a browser already knows how to present,
//! focus, and announce — never a canvas painted to look like one.
//!
//! [`Element`] is the browser's form of the tree, and the only one: the
//! HTML renderer writes it, the runtime realizes and patches it, generated
//! client views build it, WebAssembly subtrees send it, and the live mode
//! streams it. [`Realizer::element`] is the one conversion from a core
//! [`Node`]; `docs/web/dom-mapping.md` is its table in prose.
//!
//! | Node | Element |
//! |---|---|
//! | `Label` | `<span>`, or `<h1>`–`<h6>` for a heading |
//! | `Button` | `<button type="button">` |
//! | `TextInput` | `<input type="text">` |
//! | `Column`, `Row` | `<div>` (a flex container); `<ul>` of `<li>` for a list of list items; `<dialog>` for a dialog |
//! | grid | `<div>` (a grid container) |
//! | virtual list | a scrolling `<div>` with spacers for the unrealized items |
//! | `TabBar` | `role="tablist"` of `role="tab"` buttons |
//! | check box, switch, radio | `<label>` around `<input type="checkbox">` (`role="switch"`) or `radio` |
//! | slider, spinner, date | `<input type="range">`, `number`, `date` |
//! | progress | `<progress>` |
//! | select, list box | `<select>`, `<select size>` |
//! | separator, link, multi-line text, image | `<hr>`, `<a>`, `<textarea>`, `<img>` |
//! | canvas | an inline `<svg>` drawn from its draw list |
//! | native surface, foreign object | a `<div>` carrying its accessible name |

use std::collections::HashMap;

use rustnative_core::accessibility::{CheckedState, LiveRegion};
use rustnative_core::wire::key_of;
use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, AccessibleValue, Alignment, ComponentId, Control, Node,
    NodeId, SurfaceContent,
};
use serde::{Deserialize, Serialize};

use crate::css::{self, Container, Display, Flow, Layer, StyleSheet};

/// One element of the browser's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Element {
    /// Its tag name.
    #[serde(rename = "t")]
    pub tag: String,
    /// The framework key of the node it realizes, for keyed updates of a
    /// container's children; `None` for an element that is part of
    /// another's structure (a check box's `<input>`, a tab).
    #[serde(rename = "k", default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Its attributes in order; a boolean attribute has the empty value.
    #[serde(rename = "a", default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<(String, String)>,
    /// Its children.
    #[serde(rename = "c", default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Child>,
}

/// A child of an [`Element`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Child {
    /// Text.
    Text(String),
    /// An element.
    Element(Element),
}

impl Element {
    /// An element with no attributes or children.
    #[must_use]
    pub fn new(tag: &str) -> Self {
        Self { tag: tag.to_owned(), key: None, attrs: Vec::new(), children: Vec::new() }
    }

    /// Adds an attribute.
    #[must_use]
    pub fn attr(mut self, name: &str, value: impl Into<String>) -> Self {
        self.set(name, value);
        self
    }

    /// Sets an attribute, replacing any earlier value.
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.attrs.iter_mut().find(|(existing, _)| existing == name) {
            Some((_, existing)) => *existing = value,
            None => self.attrs.push((name.to_owned(), value)),
        }
    }

    /// Adds a boolean attribute when `on`.
    #[must_use]
    pub fn flag(self, name: &str, on: bool) -> Self {
        if on { self.attr(name, "") } else { self }
    }

    /// Adds a text child; an empty text adds none (the document it becomes
    /// would parse with no node there).
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        if !text.is_empty() {
            self.children.push(Child::Text(text));
        }
        self
    }

    /// Adds an element child.
    #[must_use]
    pub fn child(mut self, child: Self) -> Self {
        self.children.push(Child::Element(child));
        self
    }

    /// The value of attribute `name`, if set.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(existing, _)| existing == name).map(|(_, value)| value.as_str())
    }

    /// Every element in this subtree, this one first.
    pub fn walk(&self, visit: &mut impl FnMut(&Self)) {
        visit(self);
        for child in &self.children {
            if let Child::Element(element) = child {
                element.walk(visit);
            }
        }
    }
}

/// The role a browser gives an element on its own, so a node whose role is
/// the same needs no `role` attribute.
fn native_role(tag: &str, input_type: Option<&str>, size: bool) -> AccessibilityRole {
    match (tag, input_type) {
        ("span" | "label", _) => AccessibilityRole::Label,
        ("button", _) => AccessibilityRole::Button,
        ("input", Some("checkbox")) => AccessibilityRole::CheckBox,
        ("input", Some("radio")) => AccessibilityRole::RadioButton,
        ("input", Some("range")) => AccessibilityRole::Slider,
        ("input", Some("number")) => AccessibilityRole::SpinButton,
        ("input" | "textarea", _) => AccessibilityRole::TextInput,
        ("progress", _) => AccessibilityRole::ProgressBar,
        ("select", _) if size => AccessibilityRole::List,
        ("select", _) => AccessibilityRole::ComboBox,
        ("hr", _) => AccessibilityRole::Separator,
        ("a", _) => AccessibilityRole::Link,
        ("img", _) => AccessibilityRole::Image,
        ("ul", _) => AccessibilityRole::List,
        ("li", _) => AccessibilityRole::ListItem,
        ("dialog", _) => AccessibilityRole::Dialog,
        ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", _) => {
            let level = tag[1..].parse().unwrap_or(1);
            AccessibilityRole::Heading { level }
        }
        _ => AccessibilityRole::None,
    }
}

/// A role's ARIA name, for an element whose own role differs.
const fn aria_role(role: AccessibilityRole) -> Option<&'static str> {
    Some(match role {
        AccessibilityRole::None | AccessibilityRole::Label | AccessibilityRole::ScrollView => {
            return None;
        }
        AccessibilityRole::Button => "button",
        AccessibilityRole::TextInput => "textbox",
        AccessibilityRole::Group => "group",
        AccessibilityRole::CheckBox => "checkbox",
        AccessibilityRole::RadioButton => "radio",
        AccessibilityRole::Slider => "slider",
        AccessibilityRole::ProgressBar => "progressbar",
        AccessibilityRole::List => "list",
        AccessibilityRole::ListItem => "listitem",
        AccessibilityRole::TabList => "tablist",
        AccessibilityRole::Tab => "tab",
        AccessibilityRole::TabPanel => "tabpanel",
        AccessibilityRole::Heading { .. } => "heading",
        AccessibilityRole::Image | AccessibilityRole::Canvas => "img",
        AccessibilityRole::Link => "link",
        AccessibilityRole::Dialog => "dialog",
        AccessibilityRole::Toolbar => "toolbar",
        AccessibilityRole::Menu => "menu",
        AccessibilityRole::MenuItem => "menuitem",
        AccessibilityRole::Tree => "tree",
        AccessibilityRole::TreeItem => "treeitem",
        AccessibilityRole::Table => "table",
        AccessibilityRole::Cell => "cell",
        AccessibilityRole::Status => "status",
        AccessibilityRole::ComboBox => "combobox",
        AccessibilityRole::SpinButton => "spinbutton",
        AccessibilityRole::Separator => "separator",
        AccessibilityRole::Alert => "alert",
        // A role added to the core after this table: announced as what
        // its element is, until the table names it.
        _ => return None,
    })
}

fn is_form_control(tag: &str) -> bool {
    matches!(tag, "button" | "input" | "select" | "textarea")
}

fn is_focusable_tag(tag: &str) -> bool {
    matches!(tag, "button" | "input" | "select" | "textarea" | "a")
}

/// What a page render marks in the tree it realizes: which components are
/// interactive islands (by their index in the page data), which nodes are
/// forms that post without JavaScript, and the request-forgery token those
/// forms carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// Island owners, with each island's index.
    pub islands: HashMap<ComponentId, usize>,
    /// Form nodes, with the path each posts to.
    pub forms: HashMap<NodeId, String>,
    /// Link nodes, with where each goes.
    pub links: HashMap<NodeId, String>,
    /// The request-forgery token.
    pub csrf: String,
    /// Islands are rendered by the browser (a client-only page): their
    /// markup is left empty for it.
    pub client_only: bool,
}

/// Converts core nodes to [`Element`]s, collecting the stylesheet rules
/// they need.
pub struct Realizer<'a> {
    sheet: &'a mut StyleSheet,
    scope: String,
    keys: HashMap<(Option<ComponentId>, String), String>,
    marks: Marks,
    /// The island being realized: its owner's nodes go by their local keys
    /// under the island's scope, exactly as the browser realizes them.
    island: Option<ComponentId>,
    flows: Vec<(usize, Flow)>,
    forms: usize,
}

impl std::fmt::Debug for Realizer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Realizer").field("scope", &self.scope).finish_non_exhaustive()
    }
}

impl<'a> Realizer<'a> {
    /// A realizer writing rules into `sheet`, prefixing element ids with
    /// `scope` (empty for a page, `i0-` for its first island, …), for the
    /// tree `root`.
    #[must_use]
    pub fn new(sheet: &'a mut StyleSheet, scope: &str, root: &Node) -> Self {
        Self::with_marks(sheet, scope, root, &Marks::default())
    }

    /// A realizer for a page render's tree, with its [`Marks`].
    #[must_use]
    pub fn with_marks(sheet: &'a mut StyleSheet, scope: &str, root: &Node, marks: &Marks) -> Self {
        let mut keys = HashMap::new();
        root.visit(&mut |node, _, _| {
            let id = node.id();
            if let Some(local) = id.local_key() {
                let island = id.owner().and_then(|owner| marks.islands.get(&owner));
                keys.entry((id.owner(), local.clone())).or_insert_with(|| match island {
                    Some(index) => format!("i{index}-{local}"),
                    None => format!("{scope}{}", key_of(id)),
                });
            }
        });
        Self {
            sheet,
            scope: scope.to_owned(),
            keys,
            marks: marks.clone(),
            island: None,
            flows: Vec::new(),
            forms: 0,
        }
    }

    /// Each island realized, with how its parent lays it out (what the
    /// browser needs to realize it the same way), as JSON.
    #[must_use]
    pub fn island_flows(&self) -> Vec<(usize, serde_json::Value)> {
        self.flows.iter().map(|(index, flow)| (*index, flow.to_json())).collect()
    }

    /// `id`'s key in the element tree: inside an island, its owner's
    /// nodes go by their local keys, as the browser has them.
    fn wire(&self, id: NodeId) -> String {
        match (self.island, id.owner(), id.local_key()) {
            (Some(island), Some(owner), Some(local)) if island == owner => local,
            _ => key_of(id),
        }
    }

    /// The element id of `id`.
    #[must_use]
    pub fn element_id(&self, id: NodeId) -> String {
        format!("{}{}", self.scope, self.wire(id))
    }

    /// The element id a relationship names: the node with that local key
    /// under the same owner as the node that names it, or any node with
    /// that key.
    fn related(&self, from: NodeId, to: NodeId) -> Option<String> {
        let local = to.local_key()?;
        let owner = to.owner().or_else(|| from.owner());
        self.keys
            .get(&(owner, local.clone()))
            .or_else(|| self.keys.iter().find(|((_, key), _)| *key == local).map(|(_, id)| id))
            .cloned()
    }

    /// The element for the tree `root`, laid out as the page's root.
    #[must_use]
    pub fn root(&mut self, root: &Node) -> Element {
        self.element(root, Flow::Root)
    }

    /// The element for `node`, laid out in `flow`.
    #[must_use]
    pub fn element(&mut self, node: &Node, flow: Flow) -> Element {
        let island = node
            .id()
            .owner()
            .filter(|_| self.island.is_none())
            .and_then(|owner| self.marks.islands.get(&owner).map(|index| (owner, *index)));
        if let Some((owner, index)) = island {
            let scope = std::mem::replace(&mut self.scope, format!("i{index}-"));
            self.island = Some(owner);
            let mut element = self.kind(node, flow);
            self.island = None;
            self.scope = scope;
            element.set("data-rn-i", index.to_string());
            if self.marks.client_only {
                // The browser renders it; the element holds its place.
                element.children.clear();
                element.set("data-rn-fresh", "");
            }
            self.flows.push((index, flow));
            return element;
        }
        let form = self.marks.forms.get(&node.id()).cloned();
        if form.is_some() {
            self.forms += 1;
        }
        let mut element = self.kind(node, flow);
        if let Some(action) = form {
            self.forms -= 1;
            // A form that posts without JavaScript: the request-forgery
            // token rides along as a field.
            element.tag = "form".into();
            element.set("method", "post");
            element.set("action", action);
            let token = Element::new("input")
                .attr("type", "hidden")
                .attr("name", "_csrf")
                .attr("value", self.marks.csrf.clone());
            element.children.insert(0, Child::Element(token));
        }
        element
    }

    #[allow(clippy::too_many_lines, reason = "one arm per node kind, the mapping table itself")]
    fn kind(&mut self, node: &Node, flow: Flow) -> Element {
        let text = |value: &str| value.to_owned();
        match node {
            Node::Label(label) => {
                let (tag, native) = match node.accessibility().role() {
                    AccessibilityRole::Heading { level } if (1..=6).contains(&level) => {
                        (format!("h{level}"), node.accessibility().role())
                    }
                    _ => ("span".to_owned(), AccessibilityRole::Label),
                };
                let element = Element::new(&tag).text(text(label.text()));
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-label",
                    Container::None,
                    native,
                    Some(label.text()),
                )
            }
            Node::Button(button) => {
                let element = if self.forms > 0 {
                    // Inside a form, a button submits it, naming itself.
                    let name = self.wire(node.id());
                    Element::new("button")
                        .attr("type", "submit")
                        .attr("name", name.clone())
                        .attr("value", name)
                        .text(text(button.text()))
                } else {
                    Element::new("button").attr("type", "button").text(text(button.text()))
                };
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-button",
                    Container::None,
                    AccessibilityRole::Button,
                    Some(button.text()),
                )
            }
            Node::TextInput(input) => {
                let name = node.id().local_key().unwrap_or_default();
                let element = Element::new("input")
                    .attr("type", "text")
                    .attr("name", name)
                    .attr("value", input.value());
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-input",
                    Container::None,
                    AccessibilityRole::TextInput,
                    None,
                )
            }
            Node::TabBar(bar) => {
                let id = self.element_id(node.id());
                let mut element = Element::new("div").attr("role", "tablist");
                let selected = bar.tabs().selected();
                for (index, label) in bar.tabs().labels().iter().enumerate() {
                    let tab = Element::new("button")
                        .attr("type", "button")
                        .attr("role", "tab")
                        .attr("id", format!("{id}.{index}"))
                        .attr("aria-selected", if index == selected { "true" } else { "false" })
                        .attr("tabindex", if index == selected { "0" } else { "-1" })
                        .attr("data-i", index.to_string())
                        .text(label.clone());
                    element = element.child(tab);
                }
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-tabs",
                    Container::None,
                    AccessibilityRole::TabList,
                    None,
                )
            }
            Node::Control(_) => self.control(node, flow),
            Node::Canvas(canvas) => {
                let svg = crate::svg::draw_list(canvas.draw_list());
                let element = Element::new("div").child(svg);
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-canvas",
                    Container::None,
                    AccessibilityRole::None,
                    None,
                )
            }
            Node::Surface(surface) => {
                let mut element = Element::new("div");
                if let SurfaceContent::Foreign(kind) = surface.content() {
                    element = element.attr("data-foreign", kind.clone());
                }
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-surface",
                    Container::None,
                    AccessibilityRole::None,
                    None,
                )
            }
            Node::Column(column) => {
                if let Some(grid) = column.grid() {
                    let children = column.children();
                    let mut element = Element::new("div");
                    for child in children {
                        element = element.child(self.element(child, Flow::Grid));
                    }
                    self.decorate(
                        element,
                        node,
                        flow,
                        "rn-grid",
                        Container::Grid(grid),
                        AccessibilityRole::None,
                        None,
                    )
                } else {
                    let style = node.column_style().unwrap_or_default();
                    self.linear(
                        node,
                        column.children(),
                        flow,
                        Flow::Column(style.align_items),
                        Container::column(style),
                        "rn-col",
                    )
                }
            }
            Node::Row(row) => {
                let style = node.row_style().unwrap_or_default();
                self.linear(
                    node,
                    row.children(),
                    flow,
                    Flow::Row(style.align_items),
                    Container::row(style),
                    "rn-row",
                )
            }
        }
    }

    fn linear(
        &mut self,
        node: &Node,
        children: &[Node],
        flow: Flow,
        inner: Flow,
        container: Container<'_>,
        kind: &str,
    ) -> Element {
        let role = node.accessibility().role();
        let list = role == AccessibilityRole::List
            && !children.is_empty()
            && children
                .iter()
                .all(|child| child.accessibility().role() == AccessibilityRole::ListItem);
        let (tag, native) = match role {
            AccessibilityRole::Dialog => ("dialog", AccessibilityRole::Dialog),
            _ if list => ("ul", AccessibilityRole::List),
            _ => ("div", AccessibilityRole::None),
        };
        let mut element = Element::new(tag);
        if tag == "dialog" {
            element = element.flag("open", true);
        }
        let virtual_list = node.virtualization();
        let mut realized: Vec<usize> = Vec::new();
        let mut items: Vec<Element> = Vec::new();
        for child in children {
            let mut item = self.element(child, inner);
            if list {
                item = as_list_item(item);
            }
            if let Some(index) = child.item_index() {
                realized.push(index);
            }
            items.push(item);
        }
        let kind =
            if virtual_list.is_some() { format!("{kind} rn-vlist") } else { kind.to_owned() };
        if let Some(style) = virtual_list {
            // The unrealized items are spacers the length the list's extents
            // say they are, so the scroll range is the whole list's.
            let extents = style.extents();
            let first = realized.iter().copied().min().unwrap_or(0);
            let end = realized.iter().copied().max().map_or(0, |last| last + 1);
            let before = extents.offset_of(first);
            let after =
                extents.total().saturating_sub(extents.offset_of(end.min(style.item_count)));
            let property = match style.axis {
                rustnative_core::Axis::Vertical => "height",
                rustnative_core::Axis::Horizontal => "width",
            };
            let spacer = |sheet: &mut StyleSheet, length: u32| {
                let mut spacer = Element::new("div").attr("aria-hidden", "true");
                let class = sheet.layout(&format!("{property}:{length}px;")).unwrap_or_default();
                spacer.set("class", format!("rn rn-spacer {class}"));
                spacer
            };
            element = element.child(spacer(self.sheet, before));
            for item in items {
                element = element.child(item);
            }
            element = element.child(spacer(self.sheet, after));
            let axis = match style.axis {
                rustnative_core::Axis::Vertical => "column",
                rustnative_core::Axis::Horizontal => "row",
            };
            element = element.attr("data-vlist", format!("{axis} {}", style.item_count));
        } else {
            for item in items {
                element = element.child(item);
            }
        }
        self.decorate(element, node, flow, &kind, container, native, None)
    }

    fn control(&mut self, node: &Node, flow: Flow) -> Element {
        let Some(control) = node.control_state() else {
            return self.decorate(
                Element::new("div"),
                node,
                flow,
                "rn-control",
                Container::None,
                AccessibilityRole::None,
                None,
            );
        };
        match control {
            Control::Checkbox { label, checked } | Control::Toggle { label, on: checked } => {
                let switch = matches!(control, Control::Toggle { .. });
                let input =
                    Element::new("input").attr("type", "checkbox").flag("checked", *checked);
                let input = if switch { input.attr("role", "switch") } else { input };
                self.labelled_control(node, flow, input, label, AccessibilityRole::CheckBox)
            }
            Control::Radio { label, selected } => {
                let input = Element::new("input").attr("type", "radio").flag("checked", *selected);
                self.labelled_control(node, flow, input, label, AccessibilityRole::RadioButton)
            }
            Control::Slider { value, min, max } => {
                let element = Element::new("input")
                    .attr("type", "range")
                    .attr("min", min.to_string())
                    .attr("max", max.to_string())
                    .attr("value", value.to_string());
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::Slider,
                    None,
                )
            }
            Control::Spinner { value, min, max } => {
                let element = Element::new("input")
                    .attr("type", "number")
                    .attr("min", min.to_string())
                    .attr("max", max.to_string())
                    .attr("step", "1")
                    .attr("value", value.to_string());
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::SpinButton,
                    None,
                )
            }
            Control::Progress { percent } => {
                let mut element = Element::new("progress").attr("max", "100");
                if let Some(percent) = percent {
                    element = element.attr("value", percent.to_string());
                }
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::ProgressBar,
                    None,
                )
            }
            Control::Select { options, selected } => {
                let element = options_element(Element::new("select"), options, *selected);
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::ComboBox,
                    None,
                )
            }
            Control::ListBox { items, selected } => {
                let size = items.len().clamp(2, 10);
                let element = options_element(
                    Element::new("select").attr("size", size.to_string()),
                    items,
                    *selected,
                );
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::List,
                    None,
                )
            }
            Control::DatePicker { date } => {
                let element =
                    Element::new("input").attr("type", "date").attr("value", date.to_string());
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::TextInput,
                    None,
                )
            }
            Control::Separator => self.decorate(
                Element::new("hr"),
                node,
                flow,
                "rn-control",
                Container::None,
                AccessibilityRole::Separator,
                None,
            ),
            Control::Link { text } => {
                // A real address where the page gave one: it works with no
                // JavaScript, opens in a new tab, and can be bookmarked.
                let href =
                    self.marks.links.get(&node.id()).cloned().unwrap_or_else(|| "#".to_owned());
                let element = Element::new("a").attr("href", href).text(text.clone());
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::Link,
                    Some(text),
                )
            }
            Control::MultilineText { value } => {
                let element = Element::new("textarea").text(value.clone());
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-input",
                    Container::None,
                    AccessibilityRole::TextInput,
                    None,
                )
            }
            Control::Image { image } => {
                let element = Element::new("img")
                    .attr("src", crate::png::data_uri(image))
                    .attr("width", image.width().to_string())
                    .attr("height", image.height().to_string())
                    .attr("alt", node.accessibility().name_hint().unwrap_or_default());
                self.decorate(
                    element,
                    node,
                    flow,
                    "rn-control",
                    Container::None,
                    AccessibilityRole::Image,
                    None,
                )
            }
            _ => self.decorate(
                Element::new("div"),
                node,
                flow,
                "rn-control",
                Container::None,
                AccessibilityRole::None,
                None,
            ),
        }
    }

    /// A check box, switch, or radio: the `<input>` inside a `<label>` with
    /// its text, so clicking the text toggles it and the text names it. The
    /// label is the laid-out element; the input carries the id and the
    /// accessibility.
    fn labelled_control(
        &mut self,
        node: &Node,
        flow: Flow,
        input: Element,
        label: &str,
        native: AccessibilityRole,
    ) -> Element {
        let mut input = input.attr("id", self.element_id(node.id()));
        if native == AccessibilityRole::RadioButton {
            // Arrow keys move within a radio group: the radios of one
            // container are one group.
            let group = node
                .id()
                .owner()
                .filter(|owner| Some(*owner) != self.island)
                .map_or_else(|| "r".to_owned(), |owner| format!("r{}", owner.get()));
            input.set("name", format!("{}{group}", self.scope));
        }
        if self.forms > 0 && native == AccessibilityRole::CheckBox {
            input.set("name", self.wire(node.id()));
        }
        if node.is_disabled() {
            input.set("disabled", "");
        }
        self.accessibility(&mut input, node, node.accessibility(), native, Some(label));
        let wrapper = Element::new("label").child(input).text(label.to_owned());
        let mut wrapper = self.classes(
            wrapper,
            node,
            flow,
            "rn-control rn-check",
            &Container::None,
            Display::InlineFlex,
        );
        wrapper.key = Some(self.wire(node.id()));
        wrapper
    }

    #[allow(clippy::too_many_arguments, reason = "the mapping's inputs for one element")]
    fn decorate(
        &mut self,
        element: Element,
        node: &Node,
        flow: Flow,
        kind: &str,
        container: Container<'_>,
        native: AccessibilityRole,
        visible_text: Option<&str>,
    ) -> Element {
        let mut element = element;
        let local = node.id().local_key();
        if local.is_some() {
            element.set("id", self.element_id(node.id()));
        }
        element.key = Some(self.wire(node.id()));
        let display = match container {
            Container::Grid(_) => Display::Grid,
            Container::Linear { .. } => Display::Flex,
            Container::None => Display::Element,
        };
        let mut element = self.classes(element, node, flow, kind, &container, display);
        if node.is_disabled() {
            if is_form_control(&element.tag) {
                element.set("disabled", "");
            } else {
                element.set("aria-disabled", "true");
            }
        }
        let native = if element.tag == "input" || element.tag == "select" {
            native_role(&element.tag, element.get("type"), element.get("size").is_some())
        } else {
            native
        };
        self.accessibility(&mut element, node, node.accessibility(), native, visible_text);
        element
    }

    /// The node's classes, `hidden`, `dir`, and framework attributes.
    fn classes(
        &mut self,
        mut element: Element,
        node: &Node,
        flow: Flow,
        kind: &str,
        container: &Container<'_>,
        display: Display,
    ) -> Element {
        let mut classes = vec!["rn".to_owned(), kind.to_owned()];
        let layout = node.layout();
        if let Some(class) = self.sheet.layout(&css::layout_css(&layout, flow, container)) {
            classes.push(class);
        }
        if let Some(rules) = css::visual_rules(
            node.visual_style(),
            node.state_styles(),
            node.opacity(),
            node.cursor(),
        ) {
            classes.push(self.sheet.add(Layer::Visual, &rules));
        }
        for set in node.declarations() {
            self.sheet.note_tokens(*set);
            if let Some(rules) = css::declaration_rules(*set) {
                classes.push(self.sheet.add(Layer::Declarations, &rules));
            }
        }
        if let Some(rules) = css::node_rules(&layout, flow, node.declarations(), display) {
            classes.push(self.sheet.add(Layer::Node, &rules));
        }
        classes.dedup();
        element.set("class", classes.join(" "));
        if node.is_hidden() {
            element.set("hidden", "");
        }
        if let Some(direction) = layout.direction {
            element.set("dir", if direction.is_rtl() { "rtl" } else { "ltr" });
        }
        if let Some(command) = node.command() {
            element.set("data-command", command.name());
        }
        if let Some(shared) = node.shared_id() {
            element.set("data-shared", self.wire(shared));
        }
        if let Some(index) = node.item_index() {
            element.set("data-index", index.to_string());
        }
        element
    }

    /// The node's accessibility, as the element's own semantics where HTML
    /// has them and ARIA where it does not.
    fn accessibility(
        &self,
        element: &mut Element,
        node: &Node,
        info: &AccessibilityInfo,
        native: AccessibilityRole,
        visible_text: Option<&str>,
    ) {
        let role = info.role();
        // A group with no name tells assistive technology nothing: a plain
        // container stays a plain element.
        let unnamed_group = role == AccessibilityRole::Group && info.name_hint().is_none();
        if role != native && !unnamed_group {
            if let Some(name) = aria_role(role) {
                element.set("role", name);
            }
            if let AccessibilityRole::Heading { level } = role {
                element.set("aria-level", level.to_string());
            }
        }
        if let Some(name) = info.name_hint() {
            if visible_text != Some(name) && element.tag != "img" {
                element.set("aria-label", name);
            }
        }
        if let Some(description) = info.description_hint() {
            element.set("aria-description", description);
        }
        if let Some(id) = info.automation_id_hint() {
            element.set("data-automation-id", id);
        }
        let natively_focusable = is_focusable_tag(&element.tag);
        if info.is_focusable() && !natively_focusable && element.get("tabindex").is_none() {
            element.set("tabindex", "0");
        } else if !info.is_focusable()
            && natively_focusable
            && element.tag != "input"
            && element.tag != "select"
            && element.tag != "textarea"
        {
            element.set("tabindex", "-1");
        }
        let native_value = matches!(element.tag.as_str(), "progress" | "select" | "textarea")
            || (element.tag == "input"
                && matches!(element.get("type"), Some("range" | "number" | "text" | "date")));
        match info.value() {
            Some(AccessibleValue::Range { min, max, current, .. }) if !native_value => {
                element.set("aria-valuemin", css::fixed3(min.get()));
                element.set("aria-valuemax", css::fixed3(max.get()));
                element.set("aria-valuenow", css::fixed3(current.get()));
            }
            Some(AccessibleValue::Text(text)) if !native_value => {
                element.set("aria-valuetext", text.clone());
            }
            _ => {}
        }
        let native_check =
            element.tag == "input" && matches!(element.get("type"), Some("checkbox" | "radio"));
        if let Some(checked) = info.checked_state() {
            if !native_check {
                element.set(
                    "aria-checked",
                    match checked {
                        CheckedState::Checked => "true",
                        CheckedState::Unchecked => "false",
                        CheckedState::Mixed => "mixed",
                    },
                );
            }
        }
        if let Some(expanded) = info.expanded_state() {
            element.set("aria-expanded", expanded.to_string());
        }
        if let Some(selected) = info.selected_state() {
            element.set("aria-selected", selected.to_string());
        }
        if info.is_read_only() {
            if is_form_control(&element.tag) {
                element.set("readonly", "");
            } else {
                element.set("aria-readonly", "true");
            }
        }
        if info.is_required() {
            if is_form_control(&element.tag) {
                element.set("required", "");
            } else {
                element.set("aria-required", "true");
            }
        }
        if info.is_busy() {
            element.set("aria-busy", "true");
        }
        match info.live_region() {
            LiveRegion::Off => {}
            LiveRegion::Polite => element.set("aria-live", "polite"),
            LiveRegion::Assertive => element.set("aria-live", "assertive"),
        }
        if let Some((index, size)) = info.position() {
            element.set("aria-posinset", index.to_string());
            element.set("aria-setsize", size.to_string());
        }
        let from = node.id();
        if let Some(label) = info.labelled_by_node().and_then(|to| self.related(from, to)) {
            element.set("aria-labelledby", label);
        }
        let ids = |nodes: &[NodeId]| -> Vec<String> {
            nodes.iter().filter_map(|to| self.related(from, *to)).collect()
        };
        let described = ids(info.described_by_nodes());
        if !described.is_empty() {
            element.set("aria-describedby", described.join(" "));
        }
        let controls = ids(info.controls_nodes());
        if !controls.is_empty() {
            element.set("aria-controls", controls.join(" "));
        }
    }
}

fn options_element(mut select: Element, options: &[String], selected: Option<usize>) -> Element {
    if selected.is_none() {
        // Nothing chosen: an empty, unselectable first option, so the
        // browser does not show the first real one as chosen.
        select = select.child(
            Element::new("option")
                .attr("value", "")
                .flag("selected", true)
                .flag("disabled", true)
                .flag("hidden", true),
        );
    }
    for (index, option) in options.iter().enumerate() {
        select = select.child(
            Element::new("option")
                .attr("value", index.to_string())
                .flag("selected", selected == Some(index))
                .text(option.clone()),
        );
    }
    select
}

/// A child of a `<ul>`: an element that can be a list item itself becomes
/// one; any other is wrapped in one.
fn as_list_item(mut item: Element) -> Element {
    if matches!(item.tag.as_str(), "span" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
        "li".clone_into(&mut item.tag);
        item.attrs.retain(|(name, value)| !(name == "role" && value == "listitem"));
        return item;
    }
    let key = item.key.take();
    let mut wrapper = Element::new("li").attr("class", "rn").child(item);
    wrapper.key = key;
    wrapper
}

/// The flow a column's children are laid out in, for a caller realizing
/// one child outside a whole tree.
#[must_use]
pub const fn column_flow(align_items: Alignment) -> Flow {
    Flow::Column(align_items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustnative_core::{AccessibilityInfo, AccessibilityRole, CalendarDate, rsx};

    fn realize(node: &Node) -> (Element, StyleSheet) {
        let mut sheet = StyleSheet::new();
        let element = Realizer::new(&mut sheet, "", node).root(node);
        (element, sheet)
    }

    #[test]
    fn a_label_is_a_span_and_a_heading_is_a_heading() {
        let (label, _) = realize(&Node::label("title", "Notes"));
        assert_eq!(label.tag, "span");
        assert_eq!(label.children, vec![Child::Text("Notes".into())]);
        let heading = Node::label("title", "Notes")
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::Heading { level: 2 }));
        let (heading, _) = realize(&heading);
        assert_eq!(heading.tag, "h2");
        assert!(heading.get("role").is_none(), "an h2 needs no role");
    }

    #[test]
    fn both_syntaxes_realize_the_same_element() {
        let builder =
            Node::column("root", [Node::button("go", "Go"), Node::text_input("name", "Ada")]);
        let markup = rsx! {
            <Column key="root">
                <Button key="go" text="Go" />
                <TextInput key="name" value="Ada" />
            </Column>
        };
        assert_eq!(realize(&builder), realize(&markup));
    }

    #[test]
    fn a_list_of_list_items_is_a_ul() {
        let items = ["one", "two"].map(|text| {
            Node::label(text, text)
                .with_accessibility(AccessibilityInfo::new(AccessibilityRole::ListItem))
        });
        let list = Node::column("list", items)
            .with_accessibility(AccessibilityInfo::new(AccessibilityRole::List));
        let (element, _) = realize(&list);
        assert_eq!(element.tag, "ul");
        let Child::Element(first) = &element.children[0] else { panic!("an element") };
        assert_eq!(first.tag, "li");
        assert!(first.get("role").is_none());
    }

    #[test]
    fn controls_are_their_form_elements() {
        let (check, _) = realize(&Node::checkbox("agree", "I agree", true));
        assert_eq!(check.tag, "label");
        let Child::Element(input) = &check.children[0] else { panic!("an input") };
        assert_eq!(
            (input.get("type"), input.get("checked"), input.get("id")),
            (Some("checkbox"), Some(""), Some("agree"))
        );
        let (date, _) =
            realize(&Node::date_picker("when", CalendarDate::new(2026, 9, 27).expect("a date")));
        assert_eq!(date.get("value"), Some("2026-09-27"));
        let (toggle, _) = realize(&Node::toggle("wifi", "Wi-Fi", false));
        let Child::Element(input) = &toggle.children[0] else { panic!("an input") };
        assert_eq!(input.get("role"), Some("switch"));
    }

    #[test]
    fn disabled_hidden_and_relationships() {
        let tree = Node::column(
            "form",
            [
                Node::label("caption", "Volume"),
                Node::slider("volume", 3, 0, 10)
                    .with_accessibility(
                        AccessibilityInfo::new(AccessibilityRole::Slider).labelled_by("caption"),
                    )
                    .disabled(true),
                Node::label("gone", "x").hidden(true),
            ],
        );
        let (element, _) = realize(&tree);
        let Child::Element(slider) = &element.children[1] else { panic!() };
        assert_eq!(slider.get("aria-labelledby"), Some("caption"));
        assert_eq!(slider.get("disabled"), Some(""));
        let Child::Element(gone) = &element.children[2] else { panic!() };
        assert_eq!(gone.get("hidden"), Some(""));
    }
}
