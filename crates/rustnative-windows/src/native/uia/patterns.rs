//! UI Automation control patterns, answered from the portable model.
//!
//! One object, [`Patterns`], implements every pattern this backend
//! supports; a provider hands it out (cast to the one interface asked for)
//! only when the target's semantics actually support that pattern — see
//! [`supports`]. Every *operation* a pattern offers turns into an
//! [`AccessibleAction`] delivered to the owning component, which decides
//! what it means; every *getter* reads the node's current semantics. The
//! component is the source of truth: after a toggle, `ToggleState` reports
//! whatever the component rendered next, not what the pattern assumed.

use rustnative_core::{AccessibleAction, AccessibleActionKind, AccessibleValue, NodeKind, Scalar};
use windows::Win32::UI::Accessibility::{
    ExpandCollapseState, IExpandCollapseProvider, IInvokeProvider, IRangeValueProvider,
    IRawElementProviderSimple, IScrollItemProvider, ISelectionItemProvider, IToggleProvider,
    IValueProvider, ToggleState, UIA_ExpandCollapsePatternId, UIA_InvokePatternId, UIA_PATTERN_ID,
    UIA_RangeValuePatternId, UIA_ScrollItemPatternId, UIA_SelectionItemPatternId,
    UIA_TogglePatternId, UIA_ValuePatternId,
};
use windows::core::{BOOL, BSTR, IUnknown, Interface, PCWSTR, Result};

use super::properties::{expand_collapse_state, toggle_state};
use super::target::{Resolved, Target, invalid_operation};

/// Whether `resolved` supports `pattern`, for a target that is `native`
/// (backed by a system control whose own proxy may already provide it).
///
/// A system control's own implementation always wins over a re-implemented
/// one: a native button already invokes (and that invocation arrives as an
/// ordinary click), and a native edit control already exposes and edits its
/// value. Offering a second implementation would make assistive technology
/// bypass the control's real behavior.
pub(crate) fn supports(resolved: &Resolved, native_proxy: bool, pattern: UIA_PATTERN_ID) -> bool {
    let info = &resolved.info;
    match pattern {
        UIA_InvokePatternId => {
            info.supports(AccessibleActionKind::Invoke)
                && !(native_proxy && resolved.kind == NodeKind::Button)
        }
        UIA_ValuePatternId => {
            let textual = matches!(info.value(), Some(AccessibleValue::Text(_)))
                || (info.value().is_none() && info.supports(AccessibleActionKind::SetValue));
            textual && !(native_proxy && resolved.kind == NodeKind::TextInput)
        }
        UIA_RangeValuePatternId => matches!(info.value(), Some(AccessibleValue::Range { .. })),
        UIA_TogglePatternId => info.checked_state().is_some(),
        UIA_ExpandCollapsePatternId => info.expanded_state().is_some(),
        UIA_SelectionItemPatternId => info.selected_state().is_some(),
        UIA_ScrollItemPatternId => info.supports(AccessibleActionKind::ScrollIntoView),
        _ => false,
    }
}

/// The pattern object for `target`, as the interface `pattern` names.
pub(crate) fn provider(target: Target, pattern: UIA_PATTERN_ID) -> Option<IUnknown> {
    let object = Patterns { target };
    let unknown = match pattern {
        UIA_InvokePatternId => IInvokeProvider::from(object).cast::<IUnknown>(),
        UIA_ValuePatternId => IValueProvider::from(object).cast::<IUnknown>(),
        UIA_RangeValuePatternId => IRangeValueProvider::from(object).cast::<IUnknown>(),
        UIA_TogglePatternId => IToggleProvider::from(object).cast::<IUnknown>(),
        UIA_ExpandCollapsePatternId => IExpandCollapseProvider::from(object).cast::<IUnknown>(),
        UIA_SelectionItemPatternId => ISelectionItemProvider::from(object).cast::<IUnknown>(),
        UIA_ScrollItemPatternId => IScrollItemProvider::from(object).cast::<IUnknown>(),
        _ => return None,
    };
    unknown.ok()
}

/// Requires `action` to have been declared, then delivers it.
fn perform(target: &Target, action: AccessibleAction) -> Result<()> {
    let resolved = target.resolve()?;
    if !resolved.info.supports(action.kind()) || resolved.disabled {
        return Err(invalid_operation());
    }
    target.act(action)
}

fn range(resolved: &Resolved) -> Result<(f64, f64, f64, f64)> {
    match resolved.info.value() {
        Some(AccessibleValue::Range { min, max, current, step }) => Ok((
            f64::from(min.get()),
            f64::from(max.get()),
            f64::from(current.get()),
            f64::from(step.get()),
        )),
        _ => Err(invalid_operation()),
    }
}

/// See the module documentation. Kept in its own module so the lint
/// exceptions `windows::core::implement`'s generated code needs stay scoped.
#[allow(
    clippy::ref_as_ptr,
    clippy::inline_always,
    clippy::wildcard_imports,
    reason = "`windows::core::implement` generates this module's COM plumbing; the module is \
              a thin COM face on its parent and speaks its parent's whole vocabulary"
)]
mod com {
    use super::*;
    use windows::Win32::UI::Accessibility::{
        IExpandCollapseProvider_Impl, IInvokeProvider_Impl, IRangeValueProvider_Impl,
        IScrollItemProvider_Impl, ISelectionItemProvider_Impl, IToggleProvider_Impl,
        IValueProvider_Impl,
    };
    use windows::core::implement;

    // `Agile = false`: `#[implement]` objects are agile by default —
    // they answer `IAgileObject` and aggregate the free-threaded marshaler,
    // so COM hands callers in *other* apartments a direct pointer and they
    // call in on their own threads. This object reaches the window's
    // `Runtime`, which belongs to the UI thread alone, so it must be
    // marshaled into the UI thread's single-threaded apartment instead.
    #[implement(
        IInvokeProvider,
        IValueProvider,
        IRangeValueProvider,
        IToggleProvider,
        IExpandCollapseProvider,
        ISelectionItemProvider,
        IScrollItemProvider,
        Agile = false
    )]
    pub(crate) struct Patterns {
        pub(crate) target: Target,
    }

    impl IInvokeProvider_Impl for Patterns_Impl {
        fn Invoke(&self) -> Result<()> {
            perform(&self.target, AccessibleAction::Invoke)
        }
    }

    impl IValueProvider_Impl for Patterns_Impl {
        fn SetValue(&self, value: &PCWSTR) -> Result<()> {
            // SAFETY: UI Automation passes a valid NUL-terminated string for
            // the duration of this call.
            let value = unsafe { value.to_string() }.map_err(|_| invalid_operation())?;
            let resolved = self.target.resolve()?;
            if resolved.info.is_read_only() {
                return Err(invalid_operation());
            }
            perform(&self.target, AccessibleAction::SetValue(value))
        }

        fn Value(&self) -> Result<BSTR> {
            let resolved = self.target.resolve()?;
            Ok(match resolved.info.value() {
                Some(AccessibleValue::Text(text)) => BSTR::from(text.as_str()),
                _ => BSTR::new(),
            })
        }

        fn IsReadOnly(&self) -> Result<BOOL> {
            let resolved = self.target.resolve()?;
            let editable = resolved.info.supports(AccessibleActionKind::SetValue)
                && !resolved.info.is_read_only()
                && !resolved.disabled;
            Ok(BOOL::from(!editable))
        }
    }

    impl IRangeValueProvider_Impl for Patterns_Impl {
        fn SetValue(&self, value: f64) -> Result<()> {
            let resolved = self.target.resolve()?;
            let (min, max, ..) = range(&resolved)?;
            if resolved.info.is_read_only() {
                return Err(invalid_operation());
            }
            if !(min..=max).contains(&value) {
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_INVALIDARG,
                ));
            }
            // A range value's precision is `f32` throughout the portable
            // model; UI Automation's `f64` narrows to it here.
            #[allow(clippy::cast_possible_truncation)]
            let narrowed = value as f32;
            perform(&self.target, AccessibleAction::SetRangeValue(Scalar::new(narrowed)))
        }

        fn Value(&self) -> Result<f64> {
            range(&self.target.resolve()?).map(|(_, _, current, _)| current)
        }

        fn IsReadOnly(&self) -> Result<BOOL> {
            let resolved = self.target.resolve()?;
            Ok(BOOL::from(
                resolved.info.is_read_only()
                    || resolved.disabled
                    || !resolved.info.supports(AccessibleActionKind::SetValue),
            ))
        }

        fn Maximum(&self) -> Result<f64> {
            range(&self.target.resolve()?).map(|(_, max, ..)| max)
        }

        fn Minimum(&self) -> Result<f64> {
            range(&self.target.resolve()?).map(|(min, ..)| min)
        }

        fn LargeChange(&self) -> Result<f64> {
            // The model has one step size; a "large" change is ten of them,
            // the proportion native Windows sliders use by default.
            range(&self.target.resolve()?).map(|(.., step)| step * 10.0)
        }

        fn SmallChange(&self) -> Result<f64> {
            range(&self.target.resolve()?).map(|(.., step)| step)
        }
    }

    impl IToggleProvider_Impl for Patterns_Impl {
        fn Toggle(&self) -> Result<()> {
            perform(&self.target, AccessibleAction::Toggle)
        }

        fn ToggleState(&self) -> Result<ToggleState> {
            let resolved = self.target.resolve()?;
            resolved.info.checked_state().map(toggle_state).ok_or_else(invalid_operation)
        }
    }

    impl IExpandCollapseProvider_Impl for Patterns_Impl {
        fn Expand(&self) -> Result<()> {
            perform(&self.target, AccessibleAction::Expand)
        }

        fn Collapse(&self) -> Result<()> {
            perform(&self.target, AccessibleAction::Collapse)
        }

        fn ExpandCollapseState(&self) -> Result<ExpandCollapseState> {
            let resolved = self.target.resolve()?;
            resolved.info.expanded_state().map(expand_collapse_state).ok_or_else(invalid_operation)
        }
    }

    impl ISelectionItemProvider_Impl for Patterns_Impl {
        fn Select(&self) -> Result<()> {
            perform(&self.target, AccessibleAction::Select)
        }

        fn AddToSelection(&self) -> Result<()> {
            // The portable model has one selection operation; adding to a
            // multiple selection is the component's decision to make from it.
            perform(&self.target, AccessibleAction::Select)
        }

        fn RemoveFromSelection(&self) -> Result<()> {
            // There is no portable "deselect" action; refusing is UI
            // Automation's documented response for an unsupported change.
            Err(invalid_operation())
        }

        fn IsSelected(&self) -> Result<BOOL> {
            let resolved = self.target.resolve()?;
            Ok(BOOL::from(resolved.info.selected_state().unwrap_or(false)))
        }

        fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
            // No portable container relationship exists for selection yet;
            // `NULL` is the documented answer for "not in a container".
            Err(windows::core::Error::empty())
        }
    }

    impl IScrollItemProvider_Impl for Patterns_Impl {
        fn ScrollIntoView(&self) -> Result<()> {
            perform(&self.target, AccessibleAction::ScrollIntoView)
        }
    }
}

use com::Patterns;
