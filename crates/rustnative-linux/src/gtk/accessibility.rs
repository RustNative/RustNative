//! The portable accessibility model on GTK's accessibility API, which GTK
//! carries to AT-SPI2 itself.
//!
//! | Portable | GTK (`GtkAccessible`) | AT-SPI2 |
//! |---|---|---|
//! | role | `accessible-role` (fixed at construction) | `GetRole` |
//! | name, description | `LABEL`, `DESCRIPTION` properties | `Name`, `Description` |
//! | range value | `VALUE_MIN`/`MAX`/`NOW` | `Value` interface |
//! | text value | `VALUE_TEXT` | `Value.Text` |
//! | checked, expanded, selected, busy | states | `GetState` |
//! | read-only, required, heading level, position in set | properties | attributes and states |
//! | labelled-by, described-by, controls | relations | `GetRelationSet` |
//! | automation id | the widget's buildable id | `AccessibleId` |
//! | live region | `gtk_accessible_announce` on change | an `Announcement` event |
//! | Invoke on a custom element | the container's activate signal | `Action` "activate" |
//!
//! Driven by the portable `AccessibilityTree` after every render, and only
//! for what changed since the last one.

use std::collections::HashMap;

use gtk::AccessibleRole as Gtk;
use gtk::prelude::*;
use gtk::{accessible, glib};
use rustnative_core::{
    AccessibilityRole, AccessibilityTree, AccessibleAction, AccessibleValue, CheckedState, Event,
    LiveRegion, NodeId, NodeKind, Relation, Scalar, TreeNode, TreeSnapshot, WindowId,
};

use super::backend::{Work, post};
use super::rendering::{HostObject, NativeRegistry};
use super::virtual_accessible::{Elements, RnVirtual};

/// The GTK role for a portable role.
pub(crate) const fn gtk_role(role: AccessibilityRole) -> Gtk {
    match role {
        AccessibilityRole::Label => Gtk::Label,
        AccessibilityRole::Button => Gtk::Button,
        AccessibilityRole::TextInput => Gtk::TextBox,
        AccessibilityRole::CheckBox => Gtk::Checkbox,
        AccessibilityRole::RadioButton => Gtk::Radio,
        AccessibilityRole::Slider => Gtk::Slider,
        AccessibilityRole::ProgressBar => Gtk::ProgressBar,
        AccessibilityRole::List => Gtk::List,
        AccessibilityRole::ListItem => Gtk::ListItem,
        AccessibilityRole::TabList => Gtk::TabList,
        AccessibilityRole::Tab => Gtk::Tab,
        AccessibilityRole::TabPanel => Gtk::TabPanel,
        AccessibilityRole::Heading { .. } => Gtk::Heading,
        AccessibilityRole::Image => Gtk::Img,
        AccessibilityRole::Link => Gtk::Link,
        AccessibilityRole::Dialog => Gtk::Dialog,
        // A custom-drawn surface is a group of its virtual elements; AT-SPI
        // has no role GTK exposes for "canvas".
        AccessibilityRole::Group | AccessibilityRole::Canvas | AccessibilityRole::ScrollView => {
            Gtk::Group
        }
        AccessibilityRole::Toolbar => Gtk::Toolbar,
        AccessibilityRole::Menu => Gtk::Menu,
        AccessibilityRole::MenuItem => Gtk::MenuItem,
        AccessibilityRole::Tree => Gtk::Tree,
        AccessibilityRole::TreeItem => Gtk::TreeItem,
        AccessibilityRole::Table => Gtk::Table,
        AccessibilityRole::Cell => Gtk::Cell,
        AccessibilityRole::Status => Gtk::Status,
        AccessibilityRole::ComboBox => Gtk::ComboBox,
        AccessibilityRole::SpinButton => Gtk::SpinButton,
        AccessibilityRole::Separator => Gtk::Separator,
        AccessibilityRole::Alert => Gtk::Alert,
        // A structural node, and any role a later core adds that this
        // backend has not mapped yet: exposed as a generic element, which
        // AT-SPI clients flatten.
        _ => Gtk::Generic,
    }
}

/// The role a node's widget is constructed with. GTK fixes a widget's
/// accessible role at construction, so this is also what decides whether
/// an updated node needs a new widget.
pub(crate) fn construct_role(node: &TreeNode) -> Gtk {
    let declared = node.accessibility.role();
    match node.kind {
        // The widget's own role unless the node declares another one (a
        // label that is a heading, a button that is a tab).
        NodeKind::Label if declared == AccessibilityRole::None => Gtk::Label,
        NodeKind::Button if declared == AccessibilityRole::None => Gtk::Button,
        NodeKind::TextInput if declared == AccessibilityRole::None => Gtk::TextBox,
        NodeKind::TabBar => Gtk::TabList,
        _ => gtk_role(declared),
    }
}

/// The widget whose accessible represents `object`'s node: the control
/// itself where a node is realized as a small composite (a toggle's switch,
/// a multi-line field's text view).
pub(crate) fn accessible_of(object: &HostObject) -> gtk::Widget {
    if object.kind == NodeKind::Control {
        if let Some(switch) =
            object.widget.first_child().filter(glib::object::ObjectExt::is::<gtk::Switch>)
        {
            return switch;
        }
        if let Some(view) =
            object.widget.downcast_ref::<gtk::ScrolledWindow>().and_then(gtk::ScrolledWindow::child)
        {
            return view;
        }
    }
    object.widget.clone()
}

/// What was last applied to one node, to apply only what changes.
#[derive(Debug, Clone, PartialEq)]
struct Applied {
    name: Option<String>,
    description: Option<String>,
    value: Option<AccessibleValue>,
    checked: Option<CheckedState>,
    expanded: Option<bool>,
    selected: Option<bool>,
    busy: bool,
    read_only: bool,
    required: bool,
    level: Option<u8>,
    position: Option<(u32, u32)>,
    labelled_by: Vec<NodeId>,
    described_by: Vec<NodeId>,
    controls: Vec<NodeId>,
    automation_id: Option<String>,
}

/// The accessibility state of one window.
#[derive(Debug, Default)]
pub(crate) struct AccessibilityBridge {
    applied: HashMap<NodeId, Applied>,
    /// Each node's virtual elements, and what was applied to each.
    elements: HashMap<NodeId, Elements>,
    element_applied: HashMap<NodeId, Applied>,
    /// The widget each node's properties were applied to: a replaced widget
    /// starts from nothing.
    widgets: HashMap<NodeId, gtk::Widget>,
}

impl AccessibilityBridge {
    /// Projects `snapshot` onto the realized widgets.
    pub(crate) fn commit(
        &mut self,
        snapshot: &TreeSnapshot,
        registry: &NativeRegistry,
        window: WindowId,
    ) {
        let tree = AccessibilityTree::from_snapshot(snapshot);
        self.applied.retain(|id, _| snapshot.contains(*id));
        self.widgets.retain(|id, _| snapshot.contains(*id));
        self.elements.retain(|id, _| snapshot.contains(*id));
        for node in snapshot.nodes() {
            let Some(object) = registry.get(node.id) else { continue };
            if node.foreign.is_some() {
                // A foreign widget's accessibility is its own.
                continue;
            }
            let Some(exposed) = tree.node(node.id) else { continue };
            let info = &exposed.info;
            let resolve = |relation| tree.resolve(node.id, relation);
            let next = Applied {
                name: tree.name_of(node.id),
                description: info.description_hint().map(str::to_owned),
                value: info.value().cloned(),
                checked: info.checked_state(),
                expanded: info.expanded_state(),
                selected: info.selected_state(),
                busy: info.is_busy(),
                read_only: info.is_read_only(),
                required: info.is_required(),
                level: match info.role() {
                    AccessibilityRole::Heading { level } => Some(level),
                    _ => None,
                },
                position: info.position(),
                labelled_by: resolve(Relation::LabelledBy),
                described_by: resolve(Relation::DescribedBy),
                controls: resolve(Relation::Controls),
                automation_id: info.automation_id_hint().map(str::to_owned),
            };
            let target = accessible_of(object);
            let fresh = self.widgets.get(&node.id) != Some(&target);
            let previous = if fresh { None } else { self.applied.get(&node.id) };
            if previous == Some(&next) {
                continue;
            }
            crate::mappers::apply(
                &object.widget,
                node,
                crate::mappers::MappedProperty::Accessibility,
                || apply(target.upcast_ref(), previous, &next, registry),
            );
            let live = info.live_region();
            if live != LiveRegion::Off
                && previous.is_some_and(|previous| {
                    previous.name != next.name || previous.value != next.value
                })
            {
                announce(target.upcast_ref(), &next, live);
            }
            self.widgets.insert(node.id, target);
            self.applied.insert(node.id, next);
        }
        for node in snapshot.nodes() {
            if let Some(object) = registry.get(node.id) {
                self.sync_elements(node, object, registry, window);
            }
        }
    }

    /// Creates, updates, and removes `node`'s virtual elements.
    fn sync_elements(
        &mut self,
        node: &TreeNode,
        object: &HostObject,
        registry: &NativeRegistry,
        window: WindowId,
    ) {
        let Some(host) = object.content.clone() else { return };
        let declared = node.accessibility.elements();
        if declared.is_empty() {
            if self.elements.remove(&node.id).is_some() {
                host.set_virtual_children(Vec::new());
            }
            return;
        }
        let previous = self.elements.remove(&node.id).unwrap_or_default();
        let host_widget: gtk::Widget = host.clone().upcast();
        let mut items = Vec::new();
        for element in declared {
            let role = gtk_role(element.info().role());
            let existing = previous
                .items
                .iter()
                .find(|(id, existing)| *id == element.id() && existing.role() == role)
                .map(|(_, existing)| existing.clone());
            let fresh = existing.is_none();
            let virtual_element = existing.unwrap_or_else(|| {
                let created = RnVirtual::new(&host_widget, role, element.bounds());
                let (target, id) = (node.id, element.id());
                created.connect_value_requested(move |value| {
                    #[allow(clippy::cast_possible_truncation, reason = "an accessible range value")]
                    let value = Scalar::new(value as f32);
                    post(Work::Event(
                        window,
                        Event::AccessibilityAction {
                            target,
                            element: Some(id),
                            action: AccessibleAction::SetRangeValue(value),
                        },
                    ));
                });
                created
            });
            virtual_element.set_bounds(element.bounds());
            let info = element.info();
            let next = Applied {
                name: info.name_hint().map(str::to_owned),
                description: info.description_hint().map(str::to_owned),
                value: info.value().cloned(),
                checked: info.checked_state(),
                expanded: info.expanded_state(),
                selected: info.selected_state(),
                busy: info.is_busy(),
                read_only: info.is_read_only(),
                required: info.is_required(),
                level: None,
                position: info.position(),
                labelled_by: Vec::new(),
                described_by: Vec::new(),
                controls: Vec::new(),
                automation_id: None,
            };
            let before = if fresh { None } else { self.element_applied.get(&element.id()) };
            if before != Some(&next) {
                apply(virtual_element.upcast_ref(), before, &next, registry);
                self.element_applied.insert(element.id(), next);
            }
            items.push((element.id(), virtual_element));
        }
        let elements = Elements { items };
        elements.link(&host);
        self.elements.insert(node.id, elements);
    }
}

/// Applies what differs between `previous` and `next` to `widget`.
fn apply(
    widget: &gtk::Accessible,
    previous: Option<&Applied>,
    next: &Applied,
    registry: &NativeRegistry,
) {
    let changed = |differs: &dyn Fn(&Applied) -> bool| previous.is_none_or(differs);
    let mut properties: Vec<accessible::Property<'_>> = Vec::new();
    if changed(&|previous| previous.name != next.name) {
        if let Some(name) = &next.name {
            properties.push(accessible::Property::Label(name));
        }
    }
    if changed(&|previous| previous.description != next.description) {
        if let Some(description) = &next.description {
            properties.push(accessible::Property::Description(description));
        }
    }
    if changed(&|previous| previous.value != next.value) {
        match &next.value {
            Some(AccessibleValue::Range { min, max, current, .. }) => {
                properties.push(accessible::Property::ValueMin(f64::from(min.get())));
                properties.push(accessible::Property::ValueMax(f64::from(max.get())));
                properties.push(accessible::Property::ValueNow(f64::from(current.get())));
            }
            Some(AccessibleValue::Text(text)) => {
                properties.push(accessible::Property::ValueText(text));
            }
            None => {}
        }
    }
    if changed(&|previous| previous.read_only != next.read_only) {
        properties.push(accessible::Property::ReadOnly(next.read_only));
    }
    if changed(&|previous| previous.required != next.required) {
        properties.push(accessible::Property::Required(next.required));
    }
    if changed(&|previous| previous.level != next.level) {
        if let Some(level) = next.level {
            properties.push(accessible::Property::Level(i32::from(level)));
        }
    }
    if !properties.is_empty() {
        widget.update_property(&properties);
    }

    let mut states: Vec<accessible::State> = Vec::new();
    if changed(&|previous| previous.checked != next.checked) {
        if let Some(checked) = next.checked {
            states.push(accessible::State::Checked(match checked {
                CheckedState::Checked => gtk::AccessibleTristate::True,
                CheckedState::Unchecked => gtk::AccessibleTristate::False,
                CheckedState::Mixed => gtk::AccessibleTristate::Mixed,
            }));
        }
    }
    if changed(&|previous| previous.expanded != next.expanded) {
        states.push(accessible::State::Expanded(next.expanded));
    }
    if changed(&|previous| previous.selected != next.selected) {
        states.push(accessible::State::Selected(next.selected));
    }
    if changed(&|previous| previous.busy != next.busy) {
        states.push(accessible::State::Busy(next.busy));
    }
    if !states.is_empty() {
        widget.update_state(&states);
    }

    let widgets_of = |ids: &[NodeId]| -> Vec<gtk::Widget> {
        ids.iter().filter_map(|id| registry.get(*id)).map(accessible_of).collect()
    };
    if changed(&|previous| previous.labelled_by != next.labelled_by) {
        relation(widget, gtk::AccessibleRelation::LabelledBy, &widgets_of(&next.labelled_by));
    }
    if changed(&|previous| previous.described_by != next.described_by) {
        relation(widget, gtk::AccessibleRelation::DescribedBy, &widgets_of(&next.described_by));
    }
    if changed(&|previous| previous.controls != next.controls) {
        relation(widget, gtk::AccessibleRelation::Controls, &widgets_of(&next.controls));
    }
    if changed(&|previous| previous.position != next.position) {
        if let Some((index, size)) = next.position {
            widget.update_relation(&[
                accessible::Relation::PosInSet(i32::try_from(index).unwrap_or(i32::MAX)),
                accessible::Relation::SetSize(i32::try_from(size).unwrap_or(i32::MAX)),
            ]);
        }
    }
    if changed(&|previous| previous.automation_id != next.automation_id) {
        if let (Some(id), Some(widget)) =
            (&next.automation_id, widget.downcast_ref::<gtk::Widget>())
        {
            set_automation_id(widget, id);
        }
    }
}

/// Sets (or, with no targets, clears) one relation.
fn relation(widget: &gtk::Accessible, kind: gtk::AccessibleRelation, targets: &[gtk::Widget]) {
    if targets.is_empty() {
        widget.reset_relation(kind);
        return;
    }
    let accessibles: Vec<&gtk::Accessible> =
        targets.iter().map(glib::object::Cast::upcast_ref).collect();
    widget.update_relation(&[match kind {
        gtk::AccessibleRelation::LabelledBy => accessible::Relation::LabelledBy(&accessibles),
        gtk::AccessibleRelation::DescribedBy => accessible::Relation::DescribedBy(&accessibles),
        _ => accessible::Relation::Controls(&accessibles),
    }]);
}

/// The automation id, as the buildable id GTK reports to AT-SPI as
/// `AccessibleId` — what a `GtkBuilder` file's `id` sets, and the one
/// stable, non-localized identifier the AT-SPI tree carries.
fn set_automation_id(widget: &gtk::Widget, id: &str) {
    let value = std::ffi::CString::new(id.replace('\0', "")).unwrap_or_default();
    // SAFETY: "gtk-builder-set-id" is the key `GtkWidget` keeps its buildable
    // id under, as a `char*`; the value is a NUL-terminated string GLib
    // frees with `free_c_string` when it is replaced or the object goes.
    unsafe {
        glib::gobject_ffi::g_object_set_data_full(
            widget.as_ptr().cast(),
            c"gtk-builder-set-id".as_ptr(),
            value.into_raw().cast(),
            Some(free_c_string),
        );
    }
}

unsafe extern "C" fn free_c_string(data: glib::ffi::gpointer) {
    if !data.is_null() {
        // SAFETY: `data` is the pointer `CString::into_raw` produced in
        // `set_automation_id`, handed back exactly once by GLib.
        drop(unsafe { std::ffi::CString::from_raw(data.cast()) });
    }
}

/// Announces a live region's new content.
fn announce(widget: &gtk::Accessible, next: &Applied, live: LiveRegion) {
    let text = match &next.value {
        Some(AccessibleValue::Text(text)) => Some(text.clone()),
        Some(AccessibleValue::Range { current, .. }) => Some(current.get().to_string()),
        None => None,
    }
    .or_else(|| next.name.clone());
    let Some(text) = text else { return };
    let priority = if live == LiveRegion::Assertive {
        gtk::AccessibleAnnouncementPriority::High
    } else {
        gtk::AccessibleAnnouncementPriority::Medium
    };
    widget.announce(&text, priority);
}
