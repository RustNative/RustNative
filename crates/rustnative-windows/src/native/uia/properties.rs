//! Projecting the portable accessibility model onto UI Automation
//! properties: control types, names, states, relationships.
//!
//! # What is overridden, and what is left alone
//!
//! A provider for a node backed by a *system* control (`BUTTON`, `EDIT`,
//! `STATIC`) is merged with that control's own UI Automation proxy (see
//! `provider`), and UI Automation takes a property from the proxy whenever
//! this provider answers `VT_EMPTY`. So for those nodes this module answers
//! only what the portable model says and the native control cannot know —
//! an overridden name, a description, relationships, set position, live
//! setting — and leaves everything the control already reports correctly
//! (its enabled state, its own focusability, its value) to the control.
//! For this crate's own container windows and for virtual elements there is
//! no native proxy with any semantics, so everything the model states is
//! answered here.

use rustnative_core::{
    AccessibilityRole, AccessibleActionKind, AccessibleValue, CheckedState, LiveRegion, NodeKind,
    Relation,
};
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VT_ARRAY, VT_UNKNOWN};
use windows::Win32::UI::Accessibility::{
    Assertive, ExpandCollapseState, ExpandCollapseState_Collapsed, ExpandCollapseState_Expanded,
    HeadingLevel1, LiveSetting, Off, Polite, ToggleState, ToggleState_Indeterminate,
    ToggleState_Off, ToggleState_On, UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId,
    UIA_CONTROLTYPE_ID, UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId,
    UIA_ControlTypePropertyId, UIA_ControllerForPropertyId, UIA_CustomControlTypeId,
    UIA_DataItemControlTypeId, UIA_DescribedByPropertyId, UIA_EditControlTypeId,
    UIA_GroupControlTypeId, UIA_HeadingLevelPropertyId, UIA_HelpTextPropertyId,
    UIA_HyperlinkControlTypeId, UIA_ImageControlTypeId, UIA_IsContentElementPropertyId,
    UIA_IsControlElementPropertyId, UIA_IsEnabledPropertyId, UIA_IsKeyboardFocusablePropertyId,
    UIA_IsRequiredForFormPropertyId, UIA_ItemStatusPropertyId, UIA_LabeledByPropertyId,
    UIA_ListControlTypeId, UIA_ListItemControlTypeId, UIA_LiveSettingPropertyId,
    UIA_MenuControlTypeId, UIA_MenuItemControlTypeId, UIA_NamePropertyId, UIA_PROPERTY_ID,
    UIA_PaneControlTypeId, UIA_PositionInSetPropertyId, UIA_ProgressBarControlTypeId,
    UIA_RadioButtonControlTypeId, UIA_SeparatorControlTypeId, UIA_SizeOfSetPropertyId,
    UIA_SliderControlTypeId, UIA_SpinnerControlTypeId, UIA_StatusBarControlTypeId,
    UIA_TabControlTypeId, UIA_TabItemControlTypeId, UIA_TableControlTypeId, UIA_TextControlTypeId,
    UIA_ToolBarControlTypeId, UIA_TreeControlTypeId, UIA_TreeItemControlTypeId,
    UIA_WindowControlTypeId,
};
use windows::core::{BSTR, IUnknown, Interface};

use super::super::runtime::Runtime;
use super::target::{Resolved, Target};

/// The UI Automation control type for a portable role, or `None` for
/// [`AccessibilityRole::None`] (structure, not an element) and any role a
/// future `rustnative-core` adds that this backend has not mapped yet.
pub(crate) fn control_type(role: AccessibilityRole) -> Option<UIA_CONTROLTYPE_ID> {
    Some(match role {
        AccessibilityRole::Label | AccessibilityRole::Heading { .. } | AccessibilityRole::Alert => {
            UIA_TextControlTypeId
        }
        AccessibilityRole::Button => UIA_ButtonControlTypeId,
        AccessibilityRole::TextInput => UIA_EditControlTypeId,
        AccessibilityRole::Group => UIA_GroupControlTypeId,
        AccessibilityRole::CheckBox => UIA_CheckBoxControlTypeId,
        AccessibilityRole::RadioButton => UIA_RadioButtonControlTypeId,
        AccessibilityRole::Slider => UIA_SliderControlTypeId,
        AccessibilityRole::ProgressBar => UIA_ProgressBarControlTypeId,
        AccessibilityRole::List => UIA_ListControlTypeId,
        AccessibilityRole::ListItem => UIA_ListItemControlTypeId,
        AccessibilityRole::TabList => UIA_TabControlTypeId,
        AccessibilityRole::Tab => UIA_TabItemControlTypeId,
        AccessibilityRole::TabPanel | AccessibilityRole::ScrollView => UIA_PaneControlTypeId,
        AccessibilityRole::Image => UIA_ImageControlTypeId,
        AccessibilityRole::Link => UIA_HyperlinkControlTypeId,
        AccessibilityRole::Dialog => UIA_WindowControlTypeId,
        AccessibilityRole::Canvas => UIA_CustomControlTypeId,
        AccessibilityRole::Toolbar => UIA_ToolBarControlTypeId,
        AccessibilityRole::Menu => UIA_MenuControlTypeId,
        AccessibilityRole::MenuItem => UIA_MenuItemControlTypeId,
        AccessibilityRole::Tree => UIA_TreeControlTypeId,
        AccessibilityRole::TreeItem => UIA_TreeItemControlTypeId,
        AccessibilityRole::Table => UIA_TableControlTypeId,
        AccessibilityRole::Cell => UIA_DataItemControlTypeId,
        AccessibilityRole::Status => UIA_StatusBarControlTypeId,
        AccessibilityRole::ComboBox => UIA_ComboBoxControlTypeId,
        AccessibilityRole::SpinButton => UIA_SpinnerControlTypeId,
        AccessibilityRole::Separator => UIA_SeparatorControlTypeId,
        // `AccessibilityRole::None`, and roles this backend has not mapped.
        _ => return None,
    })
}

/// Whether the target's node is a system control whose own proxy already
/// answers the basics (see the module documentation).
pub(crate) fn has_native_proxy(target: &Target, resolved: &Resolved) -> bool {
    target.is_native()
        && matches!(
            resolved.kind,
            NodeKind::Label | NodeKind::Button | NodeKind::TextInput | NodeKind::Control
        )
}

pub(crate) fn toggle_state(state: CheckedState) -> ToggleState {
    match state {
        CheckedState::Unchecked => ToggleState_Off,
        CheckedState::Checked => ToggleState_On,
        CheckedState::Mixed => ToggleState_Indeterminate,
    }
}

pub(crate) fn expand_collapse_state(expanded: bool) -> ExpandCollapseState {
    if expanded { ExpandCollapseState_Expanded } else { ExpandCollapseState_Collapsed }
}

fn live_setting(live: LiveRegion) -> LiveSetting {
    match live {
        LiveRegion::Off => Off,
        LiveRegion::Polite => Polite,
        LiveRegion::Assertive => Assertive,
    }
}

fn text(value: &str) -> VARIANT {
    VARIANT::from(BSTR::from(value))
}

/// A `VT_ARRAY | VT_UNKNOWN` variant holding `items`, the representation
/// UI Automation uses for element-array properties (`DescribedBy`,
/// `ControllerFor`).
fn unknown_array(items: &[IUnknown]) -> Option<VARIANT> {
    let count = u32::try_from(items.len()).ok()?;
    // SAFETY: a vector of `count` `IUnknown` slots with lower bound 0; a
    // null return is the documented allocation failure.
    let array = unsafe { SafeArrayCreateVector(VT_UNKNOWN, 0, count) };
    if array.is_null() {
        return None;
    }
    for (index, item) in (0_i32..).zip(items) {
        // SAFETY: `array` was just created with `count` elements and
        // `index < count`; `SafeArrayPutElement` AddRefs the interface it
        // is handed, so `item` keeps its own reference.
        let stored = unsafe { SafeArrayPutElement(array, &raw const index, item.as_raw()) };
        if stored.is_err() {
            // SAFETY: `array` was created above and has not been handed to
            // a variant; destroying it releases whatever was already stored.
            let _ = unsafe { windows::Win32::System::Ole::SafeArrayDestroy(array) };
            return None;
        }
    }
    Some(array_variant(array))
}

fn array_variant(array: *mut SAFEARRAY) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: writes the discriminant and the matching union member of a
    // default (empty) variant; ownership of `array` moves into the variant,
    // which releases it (and every element) when dropped.
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = windows::Win32::System::Variant::VARENUM(VT_ARRAY.0 | VT_UNKNOWN.0);
        inner.Anonymous.parray = array;
    }
    variant
}

/// The value of `property` for `target`, or `None` to let UI Automation
/// fall back to the native proxy (or report "not supported").
pub(crate) fn value(
    runtime: &mut Runtime,
    target: &Target,
    resolved: &Resolved,
    property: UIA_PROPERTY_ID,
) -> Option<VARIANT> {
    let info = &resolved.info;
    let native = has_native_proxy(target, resolved);
    let exposed = info.role() != AccessibilityRole::None;
    match property {
        UIA_ControlTypePropertyId => control_type(info.role()).map(|kind| VARIANT::from(kind.0)),
        UIA_NamePropertyId => resolved.name.as_deref().map(text),
        UIA_HelpTextPropertyId => info.description_hint().map(text),
        UIA_AutomationIdPropertyId => info.automation_id_hint().map(text),
        UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => {
            Some(VARIANT::from(exposed))
        }
        UIA_IsKeyboardFocusablePropertyId if !native => {
            let focusable = if target.is_native() {
                info.is_focusable()
            } else {
                info.supports(AccessibleActionKind::Focus)
            };
            Some(VARIANT::from(focusable && !resolved.disabled))
        }
        UIA_IsEnabledPropertyId if !native => Some(VARIANT::from(!resolved.disabled)),
        UIA_IsRequiredForFormPropertyId => info.is_required().then(|| VARIANT::from(true)),
        UIA_ItemStatusPropertyId => info.is_busy().then(|| text("Busy")),
        UIA_LiveSettingPropertyId => (info.live_region() != LiveRegion::Off)
            .then(|| VARIANT::from(live_setting(info.live_region()).0)),
        UIA_PositionInSetPropertyId => {
            info.position().and_then(|(index, _)| i32::try_from(index).ok()).map(VARIANT::from)
        }
        UIA_SizeOfSetPropertyId => {
            info.position().and_then(|(_, size)| i32::try_from(size).ok()).map(VARIANT::from)
        }
        UIA_HeadingLevelPropertyId => match info.role() {
            AccessibilityRole::Heading { level } => {
                let level = i32::from(level.clamp(1, 9)) - 1;
                Some(VARIANT::from(HeadingLevel1.0 + level))
            }
            _ => None,
        },
        UIA_LabeledByPropertyId if target.is_native() => {
            let tree = runtime.renderer.accessibility.tree();
            let label = tree.resolve(target.node, Relation::LabelledBy).first().copied()?;
            let provider = super::provider_for_node(runtime, target.window, label)?;
            Some(VARIANT::from(provider.cast::<IUnknown>().ok()?))
        }
        UIA_DescribedByPropertyId | UIA_ControllerForPropertyId if target.is_native() => {
            let relation = if property == UIA_DescribedByPropertyId {
                Relation::DescribedBy
            } else {
                Relation::Controls
            };
            let related = runtime.renderer.accessibility.tree().resolve(target.node, relation);
            if related.is_empty() {
                return None;
            }
            let providers: Vec<IUnknown> = related
                .into_iter()
                .filter_map(|id| super::provider_for_node(runtime, target.window, id))
                .filter_map(|provider| provider.cast::<IUnknown>().ok())
                .collect();
            unknown_array(&providers)
        }
        _ => None,
    }
}

/// The value of the pattern-specific state properties UI Automation reads
/// through a pattern's own getters, used when announcing that one changed.
pub(crate) fn range_current(value: Option<&AccessibleValue>) -> Option<f64> {
    match value {
        Some(AccessibleValue::Range { current, .. }) => Some(f64::from(current.get())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_portable_role_but_none_has_a_control_type_and_they_are_meaningful() {
        assert_eq!(control_type(AccessibilityRole::None), None);
        assert_eq!(control_type(AccessibilityRole::Button), Some(UIA_ButtonControlTypeId));
        assert_eq!(control_type(AccessibilityRole::Slider), Some(UIA_SliderControlTypeId));
        assert_eq!(
            control_type(AccessibilityRole::Heading { level: 2 }),
            Some(UIA_TextControlTypeId),
            "a heading is text with a heading level, per UI Automation"
        );
        assert_eq!(control_type(AccessibilityRole::Tab), Some(UIA_TabItemControlTypeId));
        assert_eq!(control_type(AccessibilityRole::TabList), Some(UIA_TabControlTypeId));
    }

    #[test]
    fn element_arrays_are_unknown_safearrays() {
        let variant = unknown_array(&[]).expect("an empty array is still an array");
        // SAFETY: reading the discriminant of a variant this module built.
        let vt = unsafe { variant.Anonymous.Anonymous.vt };
        assert_eq!(vt.0, VT_ARRAY.0 | VT_UNKNOWN.0);
    }

    #[test]
    fn check_and_expand_states_map_to_their_uia_values() {
        assert_eq!(toggle_state(CheckedState::Mixed), ToggleState_Indeterminate);
        assert_eq!(expand_collapse_state(true), ExpandCollapseState_Expanded);
    }
}
