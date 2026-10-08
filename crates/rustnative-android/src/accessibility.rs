//! The portable accessibility model on `AccessibilityNodeInfo` — what
//! TalkBack, Switch Access, and every other accessibility service read
//! (`docs/android/accessibility.md`).
//!
//! | Portable | Android |
//! |---|---|
//! | role | the class name services read (`android.widget.Button`, `SeekBar`, …), or a role description where no widget class says it |
//! | name, description | `contentDescription` (when it is not the visible text), hint text |
//! | range value | `RangeInfo`, with `ACTION_SET_PROGRESS` and scroll forward/backward |
//! | text value | `stateDescription` (API 30+) |
//! | checked, selected, expanded, busy, required | checkable/checked, selected, expand/collapse actions, the state description |
//! | heading | `setHeading` (API 28+) |
//! | read-only | not editable |
//! | position in set | `CollectionItemInfo` |
//! | labelled-by | `setLabeledBy` |
//! | automation id | `setViewIdResourceName` (what UI Automator and Espresso find it by) |
//! | live region | `accessibilityLiveRegion`, and an announcement for an assertive change |
//! | hidden | `IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS` |
//! | virtual elements | an `AccessibilityNodeProvider`'s virtual views, explored by touch |
//! | actions a service performs | `Event::AccessibilityAction`, or the native control's own action |
//!
//! Driven by the portable `AccessibilityTree` after every render, and only
//! for what changed since the last one.

use std::collections::HashMap;

use rustnative_core::{
    AccessibilityRole, AccessibleAction, AccessibleValue, CheckedState, LiveRegion, NodeId,
    NodeKind, Scalar,
};

// Flags (`RnAccess`).
const CHECKABLE: i32 = 1;
const CHECKED: i32 = 2;
const MIXED: i32 = 4;
const EXPANDABLE: i32 = 8;
const EXPANDED: i32 = 16;
const SELECTED: i32 = 32;
const BUSY: i32 = 64;
const READ_ONLY: i32 = 128;
const REQUIRED: i32 = 256;
const HEADING: i32 = 512;
const INVOKE: i32 = 1024;
const RANGE: i32 = 2048;
const SELECTABLE: i32 = 4096;
const HIDDEN: i32 = 8192;
const FOCUSABLE: i32 = 16384;
const TEXT_VALUE: i32 = 32768;

/// The class name services read for a role, and the role description to
/// add where no widget class says it.
pub(crate) const fn class_for(
    role: AccessibilityRole,
) -> (Option<&'static str>, Option<&'static str>) {
    match role {
        AccessibilityRole::Button => (Some("android.widget.Button"), None),
        AccessibilityRole::CheckBox => (Some("android.widget.CheckBox"), None),
        AccessibilityRole::RadioButton => (Some("android.widget.RadioButton"), None),
        AccessibilityRole::Slider => (Some("android.widget.SeekBar"), None),
        AccessibilityRole::ProgressBar => (Some("android.widget.ProgressBar"), None),
        AccessibilityRole::TextInput => (Some("android.widget.EditText"), None),
        AccessibilityRole::List => (Some("android.widget.ListView"), None),
        AccessibilityRole::TabList => (Some("android.widget.TabWidget"), None),
        AccessibilityRole::ComboBox => (Some("android.widget.Spinner"), None),
        AccessibilityRole::SpinButton => (Some("android.widget.NumberPicker"), None),
        AccessibilityRole::Image => (Some("android.widget.ImageView"), None),
        AccessibilityRole::Toolbar => (Some("android.widget.Toolbar"), None),
        AccessibilityRole::ScrollView => (Some("android.widget.ScrollView"), None),
        AccessibilityRole::Label
        | AccessibilityRole::Heading { .. }
        | AccessibilityRole::Status
        | AccessibilityRole::Alert => (Some("android.widget.TextView"), None),
        AccessibilityRole::Link => (Some("android.widget.TextView"), Some("Link")),
        AccessibilityRole::Tab => (Some("android.view.View"), Some("Tab")),
        AccessibilityRole::TabPanel => (Some("android.view.ViewGroup"), Some("Tab panel")),
        AccessibilityRole::Dialog => (Some("android.view.ViewGroup"), Some("Dialog")),
        AccessibilityRole::Menu => (Some("android.view.ViewGroup"), Some("Menu")),
        AccessibilityRole::MenuItem => (Some("android.view.View"), Some("Menu item")),
        AccessibilityRole::Tree => (Some("android.view.ViewGroup"), Some("Tree")),
        AccessibilityRole::TreeItem => (Some("android.view.View"), Some("Tree item")),
        AccessibilityRole::Table => (Some("android.widget.GridView"), None),
        AccessibilityRole::Cell => (Some("android.view.View"), Some("Cell")),
        AccessibilityRole::ListItem => (Some("android.view.View"), None),
        AccessibilityRole::Separator => (Some("android.view.View"), Some("Separator")),
        AccessibilityRole::Group | AccessibilityRole::Canvas => {
            (Some("android.view.ViewGroup"), None)
        }
        _ => (None, None),
    }
}

/// The role a node's view has by itself: declaring it again changes
/// nothing.
const fn natural(kind: NodeKind) -> AccessibilityRole {
    match kind {
        NodeKind::Label => AccessibilityRole::Label,
        NodeKind::Button => AccessibilityRole::Button,
        NodeKind::TextInput => AccessibilityRole::TextInput,
        NodeKind::TabBar => AccessibilityRole::TabList,
        _ => AccessibilityRole::None,
    }
}

/// What is applied to one view or element: the arguments of
/// `RnAccess.apply`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Applied {
    pub(crate) strings: [Option<String>; 7],
    pub(crate) ints: [i32; 4],
    pub(crate) floats: [f32; 3],
    pub(crate) labelled_by: Option<NodeId>,
    pub(crate) live: LiveRegion,
}

/// The arguments for a node or element with `info`, whose name is `name`.
pub(crate) fn applied(
    kind: Option<NodeKind>,
    info: &rustnative_core::AccessibilityInfo,
    name: Option<String>,
    hidden: bool,
) -> Applied {
    let role = info.role();
    let (class, role_description) =
        if kind.is_some_and(|kind| natural(kind) == role) { (None, None) } else { class_for(role) };
    let mut flags = 0;
    match info.checked_state() {
        Some(CheckedState::Checked) => flags |= CHECKABLE | CHECKED,
        Some(CheckedState::Mixed) => flags |= CHECKABLE | MIXED,
        Some(CheckedState::Unchecked) => flags |= CHECKABLE,
        None => {}
    }
    match info.expanded_state() {
        Some(true) => flags |= EXPANDABLE | EXPANDED,
        Some(false) => flags |= EXPANDABLE,
        None => {}
    }
    match info.selected_state() {
        Some(true) => flags |= SELECTABLE | SELECTED,
        Some(false) => flags |= SELECTABLE,
        None => {}
    }
    if info.is_busy() {
        flags |= BUSY;
    }
    if info.is_read_only() {
        flags |= READ_ONLY;
    }
    if info.is_required() {
        flags |= REQUIRED;
    }
    if matches!(role, AccessibilityRole::Heading { .. }) {
        flags |= HEADING;
    }
    if info.supports(rustnative_core::AccessibleActionKind::Invoke) {
        flags |= INVOKE;
    }
    if info.is_focusable() {
        flags |= FOCUSABLE;
    }
    if hidden {
        flags |= HIDDEN;
    }
    let mut floats = [0.0; 3];
    let mut state = None;
    match info.value() {
        Some(AccessibleValue::Range { min, max, current, .. }) => {
            flags |= RANGE;
            floats = [min.get(), max.get(), current.get()];
        }
        Some(AccessibleValue::Text(text)) => {
            flags |= TEXT_VALUE;
            state = Some(text.clone());
        }
        None => {}
    }
    if info.checked_state() == Some(CheckedState::Mixed) {
        state = Some("Partially checked".to_owned());
    }
    let (index, size) = info.position().map_or((-1, 0), |(index, size)| {
        (i32::try_from(index).unwrap_or(i32::MAX), i32::try_from(size).unwrap_or(i32::MAX))
    });
    let live = info.live_region();
    let pane = matches!(role, AccessibilityRole::Dialog).then(|| name.clone()).flatten();
    Applied {
        strings: [
            class.map(str::to_owned),
            role_description.map(str::to_owned),
            name,
            info.description_hint().map(str::to_owned),
            state,
            info.automation_id_hint().map(str::to_owned),
            pane,
        ],
        ints: [flags, live_code(live), index, size],
        floats,
        labelled_by: info.labelled_by_node(),
        live,
    }
}

const fn live_code(live: LiveRegion) -> i32 {
    match live {
        LiveRegion::Polite => 1,
        LiveRegion::Assertive => 2,
        LiveRegion::Off => 0,
    }
}

/// The portable action `RnAccess` reported (`EV_ACCESSIBILITY`'s code and
/// value).
pub(crate) fn action_of(code: i64, value: Option<&str>) -> Option<AccessibleAction> {
    Some(match code {
        1 => AccessibleAction::Invoke,
        2 => AccessibleAction::Increment,
        3 => AccessibleAction::Decrement,
        4 => AccessibleAction::Expand,
        5 => AccessibleAction::Collapse,
        6 => AccessibleAction::Toggle,
        7 => AccessibleAction::Select,
        8 => AccessibleAction::SetValue(value.unwrap_or_default().to_owned()),
        9 => AccessibleAction::SetRangeValue(Scalar::new(
            value.and_then(|value| value.parse().ok()).unwrap_or(0.0),
        )),
        10 => AccessibleAction::ScrollIntoView,
        11 => AccessibleAction::Focus,
        _ => return None,
    })
}

/// The accessibility state of one window.
#[derive(Debug, Default)]
pub(crate) struct AccessibilityBridge {
    applied: HashMap<NodeId, Applied>,
    /// The tag each node's view had when it was applied: a replaced view
    /// starts from nothing.
    tags: HashMap<NodeId, i32>,
    /// Each node's virtual elements, in the order their indices report.
    pub(crate) elements: HashMap<NodeId, Vec<NodeId>>,
    elements_applied: HashMap<NodeId, Vec<Applied>>,
}

#[cfg(target_os = "android")]
pub(crate) use platform::after_render;

#[cfg(target_os = "android")]
mod platform {
    use rustnative_core::{AccessibilityTree, LiveRegion, NodeId, TreeSnapshot, WindowId};

    use super::{AccessibilityBridge, Applied, applied};
    use crate::Error;
    use crate::jni_host::{Arg, Class, call_static};
    use crate::registry::WindowRegistry;
    use crate::rendering::NativeRegistry;
    use crate::units::to_px;

    impl AccessibilityBridge {
        /// Projects `snapshot` onto the realized views.
        pub(crate) fn commit(
            &mut self,
            snapshot: &TreeSnapshot,
            registry: &NativeRegistry,
            window: u64,
            density: f32,
        ) {
            let tree = AccessibilityTree::from_snapshot(snapshot);
            self.applied.retain(|id, _| snapshot.contains(*id));
            self.tags.retain(|id, _| snapshot.contains(*id));
            self.elements.retain(|id, _| snapshot.contains(*id));
            self.elements_applied.retain(|id, _| snapshot.contains(*id));
            let window_arg = i64::try_from(window).unwrap_or(0);
            for node in snapshot.nodes() {
                let Some(object) = registry.get(node.id) else { continue };
                if node.foreign.is_some() {
                    // A foreign view's accessibility is its own.
                    continue;
                }
                let Some(exposed) = tree.node(node.id) else { continue };
                let mut next =
                    applied(Some(node.kind), &exposed.info, tree.name_of(node.id), node.hidden);
                // Without an automation id of its own, a node is found by
                // its key (UI Automator's resource id).
                if next.strings[5].is_none() {
                    next.strings[5] = node.id.local_key();
                }
                let fresh = self.tags.get(&node.id) != Some(&object.tag);
                let previous = if fresh { None } else { self.applied.get(&node.id) };
                if previous != Some(&next) {
                    let view = object.view.clone();
                    let _ = crate::mappers::apply(
                        &view,
                        node,
                        crate::mappers::MappedProperty::Accessibility,
                        || {
                            apply_view(&view, window_arg, object.tag, &next)?;
                            if previous
                                .is_none_or(|previous| previous.labelled_by != next.labelled_by)
                            {
                                let label = next
                                    .labelled_by
                                    .and_then(|label| registry.get(label))
                                    .map(|label| label.view.clone());
                                let label_arg = label.as_ref().map_or(Arg::Null, Arg::Obj);
                                call_static(
                                    Class::Access,
                                    "setLabeledBy",
                                    "(Landroid/view/View;JILandroid/view/View;)V",
                                    &[
                                        Arg::Obj(&view),
                                        Arg::Long(window_arg),
                                        Arg::Int(object.tag),
                                        label_arg,
                                    ],
                                )?;
                            }
                            Ok(())
                        },
                    );
                    // An assertive region speaks its change at once; a polite
                    // one is read by the service when it gets to it.
                    if next.live == LiveRegion::Assertive
                        && previous.is_some_and(|previous| {
                            previous.strings[2] != next.strings[2]
                                || previous.strings[4] != next.strings[4]
                        })
                    {
                        let text = next.strings[4]
                            .clone()
                            .or_else(|| next.strings[2].clone())
                            .unwrap_or_default();
                        let _ = call_static(
                            Class::Access,
                            "announce",
                            "(Landroid/view/View;Ljava/lang/String;)V",
                            &[Arg::Obj(&view), Arg::Str(&text)],
                        );
                    }
                    self.applied.insert(node.id, next);
                    self.tags.insert(node.id, object.tag);
                }
                self.sync_elements(node, object, window_arg, density, fresh);
            }
        }

        fn sync_elements(
            &mut self,
            node: &rustnative_core::TreeNode,
            object: &crate::rendering::HostObject,
            window: i64,
            density: f32,
            fresh: bool,
        ) {
            let declared = node.accessibility.elements();
            let ids: Vec<NodeId> =
                declared.iter().map(rustnative_core::VirtualElement::id).collect();
            let next: Vec<Applied> = declared
                .iter()
                .map(|element| {
                    applied(
                        None,
                        element.info(),
                        element.info().name_hint().map(str::to_owned),
                        false,
                    )
                })
                .collect();
            if !fresh
                && self.elements_applied.get(&node.id) == Some(&next)
                && self.elements.get(&node.id) == Some(&ids)
            {
                // Bounds can change with the same model: compared below.
                if declared.is_empty() {
                    return;
                }
            }
            if declared.is_empty() && !self.elements.contains_key(&node.id) {
                return;
            }
            let mut strings = Vec::new();
            let mut ints = Vec::new();
            let mut bounds = Vec::new();
            let mut floats = Vec::new();
            for (element, spec) in declared.iter().zip(&next) {
                strings.extend(
                    spec.strings.iter().take(6).map(|text| text.clone().unwrap_or_default()),
                );
                ints.extend([spec.ints[0], spec.ints[2], spec.ints[3]]);
                let rect = element.bounds();
                bounds.extend([
                    to_px(rect.x, density),
                    to_px(rect.y, density),
                    to_px(rect.x + rect.width, density),
                    to_px(rect.y + rect.height, density),
                ]);
                floats.extend(spec.floats);
            }
            let _ = call_static(
                Class::Access,
                "setElements",
                "(Landroid/view/View;JI[Ljava/lang/String;[I[I[F)V",
                &[
                    Arg::Obj(&object.view),
                    Arg::Long(window),
                    Arg::Int(object.tag),
                    Arg::Strs(&strings),
                    Arg::Ints(&ints),
                    Arg::Ints(&bounds),
                    Arg::Floats(&floats),
                ],
            );
            if declared.is_empty() {
                self.elements.remove(&node.id);
                self.elements_applied.remove(&node.id);
            } else {
                self.elements.insert(node.id, ids);
                self.elements_applied.insert(node.id, next);
            }
        }
    }

    fn apply_view(
        view: &crate::jni_host::JavaRef,
        window: i64,
        tag: i32,
        next: &Applied,
    ) -> Result<(), Error> {
        // The Java side takes nulls for "not said": empty strings stand for
        // them across the array, and are turned back into nulls there.
        let strings: Vec<String> =
            next.strings.iter().map(|text| text.clone().unwrap_or_default()).collect();
        call_static(
            Class::Access,
            "apply",
            "(Landroid/view/View;JI[Ljava/lang/String;[I[F)V",
            &[
                Arg::Obj(view),
                Arg::Long(window),
                Arg::Int(tag),
                Arg::Strs(&strings),
                Arg::Ints(&next.ints),
                Arg::Floats(&next.floats),
            ],
        )
        .map(|_| ())
    }

    /// What follows a render for accessibility (nothing beyond the commit
    /// the renderer makes; kept for the window-level announcements).
    #[allow(
        clippy::unnecessary_wraps,
        reason = "the registry's after-render hooks all report failure the same way"
    )]
    pub(crate) fn after_render(
        _registry: &mut WindowRegistry,
        _window: WindowId,
    ) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::{AccessibilityInfo, AccessibleActionKind};

    use super::*;

    #[test]
    fn roles_become_the_class_names_services_read() {
        assert_eq!(class_for(AccessibilityRole::Slider).0, Some("android.widget.SeekBar"));
        assert_eq!(
            class_for(AccessibilityRole::Link),
            (Some("android.widget.TextView"), Some("Link"))
        );
        assert_eq!(class_for(AccessibilityRole::None), (None, None));
        // A button's own role is not repeated.
        let button = applied(
            Some(NodeKind::Button),
            &AccessibilityInfo::new(AccessibilityRole::Button),
            Some("Go".into()),
            false,
        );
        assert_eq!(button.strings[0], None);
        // A label declared a heading is reported as one.
        let heading = applied(
            Some(NodeKind::Label),
            &AccessibilityInfo::new(AccessibilityRole::Heading { level: 2 }),
            None,
            false,
        );
        assert_eq!(heading.ints[0] & HEADING, HEADING);
        assert_eq!(heading.strings[0].as_deref(), Some("android.widget.TextView"));
    }

    #[test]
    fn states_values_and_actions_become_flags() {
        let info = AccessibilityInfo::new(AccessibilityRole::Slider)
            .range(0.0, 10.0, 4.0, 1.0)
            .required(true)
            .position_in_set(2, 5)
            .action(AccessibleActionKind::Invoke)
            .automation_id("volume")
            .live(LiveRegion::Assertive);
        let spec = applied(Some(NodeKind::Column), &info, Some("Volume".into()), false);
        assert_eq!(spec.ints[0] & (RANGE | REQUIRED | INVOKE), RANGE | REQUIRED | INVOKE);
        assert_eq!(spec.floats.map(f32::to_bits), [0.0_f32, 10.0, 4.0].map(f32::to_bits));
        assert_eq!(spec.ints[1], 2);
        assert_eq!((spec.ints[2], spec.ints[3]), (2, 5));
        assert_eq!(spec.strings[5].as_deref(), Some("volume"));
        let check = applied(
            None,
            &AccessibilityInfo::new(AccessibilityRole::CheckBox).checked(CheckedState::Mixed),
            None,
            true,
        );
        assert_eq!(check.ints[0] & (CHECKABLE | MIXED | HIDDEN), CHECKABLE | MIXED | HIDDEN);
        assert_eq!(check.strings[4].as_deref(), Some("Partially checked"));
    }

    #[test]
    fn reported_actions_are_the_portable_ones() {
        assert_eq!(action_of(1, None), Some(AccessibleAction::Invoke));
        assert_eq!(
            action_of(9, Some("7.5")),
            Some(AccessibleAction::SetRangeValue(Scalar::new(7.5)))
        );
        assert_eq!(action_of(8, Some("hi")), Some(AccessibleAction::SetValue("hi".into())));
        assert_eq!(action_of(99, None), None);
    }
}
