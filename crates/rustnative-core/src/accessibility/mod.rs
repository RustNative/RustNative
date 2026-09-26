//! The portable accessibility model: what a node *is* to assistive
//! technology, independent of any platform's accessibility API.
//!
//! # What this covers
//!
//! Every requirement `PLAN.md`'s Milestone 26 lists, as data a backend
//! projects onto its platform's accessibility system (UI Automation on
//! Windows, `NSAccessibility`, `AccessibilityNodeInfo`, ARIA, AT-SPI):
//!
//! | Requirement | Here |
//! |---|---|
//! | roles | [`AccessibilityRole`] |
//! | names, descriptions | [`AccessibilityInfo::name`], [`AccessibilityInfo::description`] |
//! | value/state | [`AccessibleValue`], [`CheckedState`] and the other state setters |
//! | actions | [`AccessibleActionKind`] (declared) and [`AccessibleAction`] (invoked) |
//! | ranges | [`AccessibleValue::Range`] |
//! | relationships | [`AccessibilityInfo::labelled_by`] and friends |
//! | focus | [`AccessibilityInfo::focusable`] |
//! | virtualized children | [`AccessibilityInfo::position_in_set`], [`VirtualElement`] |
//!
//! plus live regions ([`LiveRegion`]) and stable automation ids.
//!
//! # Virtual elements
//!
//! A node realized as a single native object can still present several
//! semantic elements: a custom-drawn chart's bars, a canvas' hit regions, a
//! custom tab strip. [`VirtualElement`]s describe those — each with its own
//! role, name, value, actions, and bounds relative to the node — without
//! turning them into native objects. A backend exposes them as child
//! elements of the node (UI Automation fragments on Windows); an action an
//! assistive technology invokes on one arrives as
//! [`crate::Event::AccessibilityAction`] with its element id.
//!
//! # Relationship keys
//!
//! Relationship targets are written as the same component-local keys nodes
//! are, and the component runtime scopes them exactly as it scopes node
//! keys (see `crate::identity`), so `labelled_by("title")` inside a reusable
//! component refers to *that* component's `"title"`. A target that does not
//! resolve to a node in the current tree is ignored, not an error:
//! relationships are advisory metadata.

mod tree;

pub use tree::{AccessibilityTree, AccessibleNode, Relation};

use std::collections::BTreeSet;

use crate::identity::NodeId;
use crate::input::Scalar;
use crate::layout::Rect;

/// A portable accessibility role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum AccessibilityRole {
    /// No specific role; the node is not exposed as a distinct accessible
    /// element (a purely structural container).
    None,
    /// A static, non-interactive text label.
    Label,
    /// An activatable control (e.g. a push button).
    Button,
    /// An editable text field.
    TextInput,
    /// A container grouping other accessible elements.
    Group,
    /// A two- or three-state check box.
    CheckBox,
    /// One option in a set of mutually exclusive options.
    RadioButton,
    /// A control for choosing a value in a range.
    Slider,
    /// A read-only indication of progress through a range.
    ProgressBar,
    /// A list of items.
    List,
    /// An item in a [`Self::List`].
    ListItem,
    /// A strip of tabs.
    TabList,
    /// A tab in a [`Self::TabList`].
    Tab,
    /// The content a [`Self::Tab`] shows.
    TabPanel,
    /// A section heading, at `level` 1 (most important) through 9.
    Heading {
        /// The heading level, 1 through 9.
        level: u8,
    },
    /// An image or graphic.
    Image,
    /// A hyperlink.
    Link,
    /// A dialog.
    Dialog,
    /// A scrollable region.
    ScrollView,
    /// A custom-drawn surface, typically with [`VirtualElement`] children.
    Canvas,
    /// A toolbar.
    Toolbar,
    /// A menu.
    Menu,
    /// An item in a [`Self::Menu`].
    MenuItem,
    /// A hierarchical tree.
    Tree,
    /// An item in a [`Self::Tree`].
    TreeItem,
    /// A table or grid.
    Table,
    /// A cell in a [`Self::Table`].
    Cell,
    /// Status information (a status bar, a toast).
    Status,
    /// A drop-down choice of one option (Milestone 48).
    ComboBox,
    /// A number stepped up and down.
    SpinButton,
    /// A rule between groups of content.
    Separator,
    /// An important, time-sensitive message.
    Alert,
}

/// A node's value, as assistive technology reads (and may set) it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessibleValue {
    /// A textual value (a text field's content, a combo box's selection).
    Text(String),
    /// A numeric value in a range (a slider, a progress bar).
    Range {
        /// The smallest allowed value.
        min: Scalar,
        /// The largest allowed value.
        max: Scalar,
        /// The current value.
        current: Scalar,
        /// The increment one step moves by.
        step: Scalar,
    },
}

/// The checked state of a check box, toggle button, or radio button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckedState {
    /// Not checked.
    Unchecked,
    /// Checked.
    Checked,
    /// Neither — some but not all of what it summarizes is checked.
    Mixed,
}

/// How assistive technology should announce changes to a node's name or
/// value while the person is elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LiveRegion {
    /// Changes are not announced.
    #[default]
    Off,
    /// Changes are announced when the person is idle.
    Polite,
    /// Changes are announced immediately, interrupting.
    Assertive,
}

/// An operation a node *supports*, declared so assistive technology can
/// offer it. See [`AccessibleAction`] for an invocation of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AccessibleActionKind {
    /// Activate it (press a button, follow a link).
    Invoke,
    /// Step a range value up.
    Increment,
    /// Step a range value down.
    Decrement,
    /// Expand it.
    Expand,
    /// Collapse it.
    Collapse,
    /// Toggle its checked state.
    Toggle,
    /// Select it within its container.
    Select,
    /// Set its value.
    SetValue,
    /// Scroll it into view.
    ScrollIntoView,
    /// Give it keyboard focus.
    Focus,
}

/// An operation assistive technology asked a node (or one of its
/// [`VirtualElement`]s) to perform, delivered as
/// [`crate::Event::AccessibilityAction`].
///
/// Deliberately separate from the ordinary input events: a screen reader
/// "pressing" a custom control is a request the component decides how to
/// honor, not a synthesized click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessibleAction {
    /// See [`AccessibleActionKind::Invoke`].
    Invoke,
    /// See [`AccessibleActionKind::Increment`].
    Increment,
    /// See [`AccessibleActionKind::Decrement`].
    Decrement,
    /// See [`AccessibleActionKind::Expand`].
    Expand,
    /// See [`AccessibleActionKind::Collapse`].
    Collapse,
    /// See [`AccessibleActionKind::Toggle`].
    Toggle,
    /// See [`AccessibleActionKind::Select`].
    Select,
    /// Set a textual value.
    SetValue(String),
    /// Set a range value.
    SetRangeValue(Scalar),
    /// See [`AccessibleActionKind::ScrollIntoView`].
    ScrollIntoView,
    /// See [`AccessibleActionKind::Focus`].
    Focus,
}

impl AccessibleAction {
    /// The declared capability this invocation exercises.
    #[must_use]
    pub const fn kind(&self) -> AccessibleActionKind {
        match self {
            Self::Invoke => AccessibleActionKind::Invoke,
            Self::Increment => AccessibleActionKind::Increment,
            Self::Decrement => AccessibleActionKind::Decrement,
            Self::Expand => AccessibleActionKind::Expand,
            Self::Collapse => AccessibleActionKind::Collapse,
            Self::Toggle => AccessibleActionKind::Toggle,
            Self::Select => AccessibleActionKind::Select,
            Self::SetValue(_) | Self::SetRangeValue(_) => AccessibleActionKind::SetValue,
            Self::ScrollIntoView => AccessibleActionKind::ScrollIntoView,
            Self::Focus => AccessibleActionKind::Focus,
        }
    }
}

/// The semantic relationships a node declares to other nodes, by their
/// (component-scoped) identities.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Relationships {
    labelled_by: Option<NodeId>,
    described_by: Vec<NodeId>,
    controls: Vec<NodeId>,
}

/// Portable accessibility metadata attached to a node (or to a
/// [`VirtualElement`]).
///
/// # Example
///
/// ```
/// use rustnative_core::{
///     AccessibilityInfo, AccessibilityRole, AccessibleActionKind, AccessibleValue, Node,
/// };
///
/// // A custom-drawn slider: one native object, fully described.
/// let volume = Node::column("volume", []).with_accessibility(
///     AccessibilityInfo::new(AccessibilityRole::Slider)
///         .name("Volume")
///         .labelled_by("volume-caption")
///         .range(0.0, 100.0, 35.0, 5.0)
///         .action(AccessibleActionKind::SetValue)
///         .action(AccessibleActionKind::Increment)
///         .action(AccessibleActionKind::Decrement)
///         .focusable(true),
/// );
/// let info = volume.accessibility();
/// assert!(matches!(info.value(), Some(AccessibleValue::Range { .. })));
/// assert!(info.supports(AccessibleActionKind::Increment));
///
/// // The same node in markup:
/// let markup = rustnative_core::rsx! {
///     <Column
///         key="volume"
///         accessibility={AccessibilityInfo::new(AccessibilityRole::Slider)
///             .name("Volume")
///             .labelled_by("volume-caption")
///             .range(0.0, 100.0, 35.0, 5.0)
///             .action(AccessibleActionKind::SetValue)
///             .action(AccessibleActionKind::Increment)
///             .action(AccessibleActionKind::Decrement)
///             .focusable(true)}
///     ></Column>
/// };
/// assert_eq!(markup, volume);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent semantic states, not a state machine"
)]
pub struct AccessibilityInfo {
    role: AccessibilityRole,
    name: Option<String>,
    description: Option<String>,
    automation_id: Option<String>,
    focusable: bool,
    value: Option<AccessibleValue>,
    checked: Option<CheckedState>,
    expanded: Option<bool>,
    selected: Option<bool>,
    read_only: bool,
    required: bool,
    busy: bool,
    live: LiveRegion,
    position: Option<(u32, u32)>,
    actions: BTreeSet<AccessibleActionKind>,
    relationships: Relationships,
    elements: Vec<VirtualElement>,
}

impl AccessibilityInfo {
    /// Creates accessibility metadata for `role`, with nothing else set and
    /// not focusable.
    #[must_use]
    pub fn new(role: AccessibilityRole) -> Self {
        Self {
            role,
            name: None,
            description: None,
            automation_id: None,
            focusable: false,
            value: None,
            checked: None,
            expanded: None,
            selected: None,
            read_only: false,
            required: false,
            busy: false,
            live: LiveRegion::Off,
            position: None,
            actions: BTreeSet::new(),
            relationships: Relationships::default(),
            elements: Vec::new(),
        }
    }

    /// Sets the accessible name (the primary label assistive technology
    /// announces for this node).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the accessible description (supplementary detail announced
    /// after the name).
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets a stable identifier test automation can find this node by,
    /// independent of its (possibly localized) name.
    #[must_use]
    pub fn automation_id(mut self, id: impl Into<String>) -> Self {
        self.automation_id = Some(id.into());
        self
    }

    /// Sets whether this node can receive keyboard focus.
    #[must_use]
    pub const fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Sets a textual value.
    #[must_use]
    pub fn text_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(AccessibleValue::Text(value.into()));
        self
    }

    /// Sets a range value: `current` within `min..=max`, moving by `step`.
    #[must_use]
    pub fn range(mut self, min: f32, max: f32, current: f32, step: f32) -> Self {
        self.value = Some(AccessibleValue::Range {
            min: Scalar::new(min),
            max: Scalar::new(max),
            current: Scalar::new(current.clamp(min.min(max), max.max(min))),
            step: Scalar::new(step),
        });
        self
    }

    /// Sets the checked state.
    #[must_use]
    pub const fn checked(mut self, state: CheckedState) -> Self {
        self.checked = Some(state);
        self
    }

    /// Sets whether the node is expanded (only meaningful for nodes that
    /// can expand).
    #[must_use]
    pub const fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Sets whether the node is selected within its container.
    #[must_use]
    pub const fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// Marks the node's value as not editable.
    #[must_use]
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Marks the node as required (a form field that must be filled).
    #[must_use]
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Marks the node as busy (its content is being updated).
    #[must_use]
    pub const fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// Makes changes to this node's name or value announced (see
    /// [`LiveRegion`]).
    #[must_use]
    pub const fn live(mut self, live: LiveRegion) -> Self {
        self.live = live;
        self
    }

    /// Declares this node the `index`th (1-based) of `size` items in its
    /// set — what assistive technology announces as "3 of 250", and what
    /// lets a virtualized list with only a few realized items still report
    /// its true length.
    #[must_use]
    pub const fn position_in_set(mut self, index: u32, size: u32) -> Self {
        self.position = Some((index, size));
        self
    }

    /// Declares support for `action`.
    #[must_use]
    pub fn action(mut self, action: AccessibleActionKind) -> Self {
        self.actions.insert(action);
        self
    }

    /// Names the node whose text labels this one, by local key.
    #[must_use]
    pub fn labelled_by(mut self, key: impl AsRef<str>) -> Self {
        self.relationships.labelled_by = Some(NodeId::from_key(key.as_ref()));
        self
    }

    /// Adds a node whose text describes this one, by local key.
    #[must_use]
    pub fn described_by(mut self, key: impl AsRef<str>) -> Self {
        self.relationships.described_by.push(NodeId::from_key(key.as_ref()));
        self
    }

    /// Adds a node whose content or presence this one controls, by local
    /// key.
    #[must_use]
    pub fn controls(mut self, key: impl AsRef<str>) -> Self {
        self.relationships.controls.push(NodeId::from_key(key.as_ref()));
        self
    }

    /// Adds a semantic child that has no native object of its own (see
    /// [`VirtualElement`]).
    #[must_use]
    pub fn element(mut self, element: VirtualElement) -> Self {
        self.elements.push(element);
        self
    }

    /// Returns the accessible role.
    #[must_use]
    pub const fn role(&self) -> AccessibilityRole {
        self.role
    }

    /// Returns the accessible name, if one was set.
    #[must_use]
    pub fn name_hint(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the accessible description, if one was set.
    #[must_use]
    pub fn description_hint(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the automation id, if one was set.
    #[must_use]
    pub fn automation_id_hint(&self) -> Option<&str> {
        self.automation_id.as_deref()
    }

    /// Returns whether this node can receive keyboard focus.
    #[must_use]
    pub const fn is_focusable(&self) -> bool {
        self.focusable
    }

    /// Returns the value, if any.
    #[must_use]
    pub const fn value(&self) -> Option<&AccessibleValue> {
        self.value.as_ref()
    }

    /// Returns the checked state, if this node has one.
    #[must_use]
    pub const fn checked_state(&self) -> Option<CheckedState> {
        self.checked
    }

    /// Returns whether the node is expanded, if it can expand.
    #[must_use]
    pub const fn expanded_state(&self) -> Option<bool> {
        self.expanded
    }

    /// Returns whether the node is selected, if it is selectable.
    #[must_use]
    pub const fn selected_state(&self) -> Option<bool> {
        self.selected
    }

    /// Returns whether the value is read-only.
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Returns whether the node is required.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }

    /// Returns whether the node is busy.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.busy
    }

    /// Returns the live-region setting.
    #[must_use]
    pub const fn live_region(&self) -> LiveRegion {
        self.live
    }

    /// Returns `(index, size)` if the node declared its position in a set.
    #[must_use]
    pub const fn position(&self) -> Option<(u32, u32)> {
        self.position
    }

    /// Returns whether `action` is supported.
    #[must_use]
    pub fn supports(&self, action: AccessibleActionKind) -> bool {
        self.actions.contains(&action)
    }

    /// Iterates the supported actions, in a stable order.
    pub fn actions(&self) -> impl Iterator<Item = AccessibleActionKind> + '_ {
        self.actions.iter().copied()
    }

    /// Returns the node this one is labelled by, if declared.
    #[must_use]
    pub const fn labelled_by_node(&self) -> Option<NodeId> {
        self.relationships.labelled_by
    }

    /// Returns the nodes that describe this one.
    #[must_use]
    pub fn described_by_nodes(&self) -> &[NodeId] {
        &self.relationships.described_by
    }

    /// Returns the nodes this one controls.
    #[must_use]
    pub fn controls_nodes(&self) -> &[NodeId] {
        &self.relationships.controls
    }

    /// Returns the semantic children with no native object of their own.
    #[must_use]
    pub fn elements(&self) -> &[VirtualElement] {
        &self.elements
    }

    /// Finds a virtual element (at any depth) by its id.
    #[must_use]
    pub fn find_element(&self, id: NodeId) -> Option<&VirtualElement> {
        self.elements.iter().find_map(|element| element.find(id))
    }

    /// Rewrites every relationship target through `scope` — how the
    /// component runtime turns local keys into the component-scoped
    /// identities nodes are realized under.
    pub(crate) fn scope_relationships(&mut self, scope: &impl Fn(NodeId) -> NodeId) {
        let relationships = &mut self.relationships;
        relationships.labelled_by = relationships.labelled_by.map(scope);
        for id in relationships.described_by.iter_mut().chain(relationships.controls.iter_mut()) {
            *id = scope(*id);
        }
        for element in &mut self.elements {
            element.info.scope_relationships(scope);
        }
    }
}

/// A semantic element inside a node that has no native object of its own —
/// see the module documentation.
///
/// # Example
///
/// ```
/// use rustnative_core::{
///     AccessibilityInfo, AccessibilityRole, AccessibleActionKind, Node, Rect, VirtualElement,
/// };
///
/// // A chart drawn as one surface, with each bar individually accessible.
/// let chart = Node::column("chart", []).with_accessibility(
///     AccessibilityInfo::new(AccessibilityRole::Canvas)
///         .name("Sales by quarter")
///         .element(VirtualElement::new(
///             "q1",
///             AccessibilityInfo::new(AccessibilityRole::Button)
///                 .name("Q1: 1.2M")
///                 .action(AccessibleActionKind::Invoke),
///             Rect::new(0, 40, 30, 60),
///         )),
/// );
/// assert_eq!(chart.accessibility().elements().len(), 1);
///
/// // The same node in markup:
/// let bar = VirtualElement::new(
///     "q1",
///     AccessibilityInfo::new(AccessibilityRole::Button).name("Q1: 1.2M").action(AccessibleActionKind::Invoke),
///     Rect::new(0, 40, 30, 60),
/// );
/// let described = AccessibilityInfo::new(AccessibilityRole::Canvas).name("Sales by quarter").element(bar);
/// assert_eq!(rustnative_core::rsx! { <Column key="chart" accessibility={described}></Column> }, chart);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualElement {
    id: NodeId,
    info: AccessibilityInfo,
    bounds: Rect,
}

impl VirtualElement {
    /// An element identified by `key` (unique within its node), described
    /// by `info`, occupying `bounds` in the node's local coordinates.
    ///
    /// Nest elements by giving `info` elements of its own.
    #[must_use]
    pub fn new(key: impl AsRef<str>, info: AccessibilityInfo, bounds: Rect) -> Self {
        Self { id: NodeId::from_key(key.as_ref()), info, bounds }
    }

    /// The element's id — its key, interned like a node key and *not*
    /// component-scoped, since it only means something relative to its
    /// node.
    #[must_use]
    pub const fn id(&self) -> NodeId {
        self.id
    }

    /// The element's accessibility metadata (including its own children).
    #[must_use]
    pub const fn info(&self) -> &AccessibilityInfo {
        &self.info
    }

    /// The element's bounds in its node's local coordinates.
    #[must_use]
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    fn find(&self, id: NodeId) -> Option<&Self> {
        if self.id == id {
            return Some(self);
        }
        self.info.elements.iter().find_map(|element| element.find(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessibility_info_builder_round_trips() {
        let info = AccessibilityInfo::new(AccessibilityRole::Button)
            .name("Submit")
            .description("Submits the form")
            .automation_id("submit-button")
            .focusable(true);
        assert_eq!(info.role(), AccessibilityRole::Button);
        assert_eq!(info.name_hint(), Some("Submit"));
        assert_eq!(info.description_hint(), Some("Submits the form"));
        assert_eq!(info.automation_id_hint(), Some("submit-button"));
        assert!(info.is_focusable());
    }

    #[test]
    fn a_range_value_is_clamped_into_its_bounds() {
        let info = AccessibilityInfo::new(AccessibilityRole::Slider).range(0.0, 10.0, 99.0, 1.0);
        let Some(AccessibleValue::Range { current, .. }) = info.value() else {
            panic!("a range was set");
        };
        assert!((current.get() - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn states_actions_and_relationships_are_recorded() {
        let info = AccessibilityInfo::new(AccessibilityRole::CheckBox)
            .checked(CheckedState::Mixed)
            .expanded(false)
            .selected(true)
            .required(true)
            .busy(true)
            .read_only(true)
            .live(LiveRegion::Assertive)
            .position_in_set(3, 250)
            .action(AccessibleActionKind::Toggle)
            .labelled_by("caption")
            .described_by("hint")
            .controls("panel");
        assert_eq!(info.checked_state(), Some(CheckedState::Mixed));
        assert_eq!(info.expanded_state(), Some(false));
        assert_eq!(info.selected_state(), Some(true));
        assert!(info.is_required() && info.is_busy() && info.is_read_only());
        assert_eq!(info.live_region(), LiveRegion::Assertive);
        assert_eq!(info.position(), Some((3, 250)));
        assert!(info.supports(AccessibleActionKind::Toggle));
        assert!(!info.supports(AccessibleActionKind::Invoke));
        assert_eq!(info.labelled_by_node(), Some(NodeId::from_key("caption")));
        assert_eq!(info.described_by_nodes(), [NodeId::from_key("hint")]);
        assert_eq!(info.controls_nodes(), [NodeId::from_key("panel")]);
    }

    #[test]
    fn nested_virtual_elements_are_found_by_id() {
        let inner = VirtualElement::new(
            "inner",
            AccessibilityInfo::new(AccessibilityRole::Button),
            Rect::new(1, 1, 2, 2),
        );
        let outer = VirtualElement::new(
            "outer",
            AccessibilityInfo::new(AccessibilityRole::Group).element(inner),
            Rect::new(0, 0, 10, 10),
        );
        let info = AccessibilityInfo::new(AccessibilityRole::Canvas).element(outer);
        assert_eq!(
            info.find_element(NodeId::from_key("inner")).map(VirtualElement::bounds),
            Some(Rect::new(1, 1, 2, 2))
        );
        assert!(info.find_element(NodeId::from_key("missing")).is_none());
    }

    #[test]
    fn every_invoked_action_maps_to_the_capability_it_needs() {
        assert_eq!(
            AccessibleAction::SetRangeValue(Scalar::ONE).kind(),
            AccessibleActionKind::SetValue
        );
        assert_eq!(AccessibleAction::SetValue("x".into()).kind(), AccessibleActionKind::SetValue);
        assert_eq!(AccessibleAction::Toggle.kind(), AccessibleActionKind::Toggle);
    }
}
