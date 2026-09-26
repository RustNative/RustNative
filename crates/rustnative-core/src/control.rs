//! The native controls beyond text, buttons, and tabs (`PLAN.md`
//! Milestone 48): check boxes, radio buttons, switches, sliders, progress
//! bars, selects, list boxes, date pickers, spinners, separators, links,
//! multi-line text, and images.
//!
//! Each is one [`Control`] carried by a [`crate::Node::Control`] node and
//! realized by the platform's own control where it has one, so it looks,
//! behaves, and is announced like every other such control on the system.
//! Each is *controlled*: a person's change raises an event, the component
//! answers by rendering the new state, and the backend puts the control back
//! to what the node says — so a component that refuses a change keeps the
//! control where it was.
//!
//! | Control | Event | Accessibility |
//! |---|---|---|
//! | `Checkbox`, `Toggle` | [`crate::Event::Toggled`] | check box, checked state |
//! | `Radio` | [`crate::Event::Toggled`] (`on: true`) | radio button, checked state |
//! | `Slider`, `Spinner` | [`crate::Event::ValueChanged`] | slider / spin button, range value |
//! | `Progress` | — | progress bar, range value (busy when indeterminate) |
//! | `Select`, `ListBox` | [`crate::Event::SelectionChanged`] | combo box / list, the selected text |
//! | `DatePicker` | [`crate::Event::DateChanged`] | date entry, the date as text |
//! | `Link` | [`crate::Event::Click`] | link |
//! | `MultilineText` | [`crate::Event::TextChanged`] | text input |
//! | `Separator`, `Image` | — | separator / image |

use crate::accessibility::{AccessibilityInfo, CheckedState};
use crate::event::AccessibilityRole;
use crate::graphics::ImageData;

/// A day in the proleptic Gregorian calendar.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct CalendarDate {
    /// The year.
    pub year: i32,
    /// The month, 1 to 12.
    pub month: u8,
    /// The day of the month, 1 to 31.
    pub day: u8,
}

impl CalendarDate {
    /// The date `year`-`month`-`day`, or `None` when it does not exist.
    #[must_use]
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (1..=days).contains(&day).then_some(Self { year, month, day })
    }
}

impl std::fmt::Display for CalendarDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// What a [`crate::Node::Control`] is; see the [module
/// documentation](crate::control).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum Control {
    /// A check box with a label.
    Checkbox {
        /// Its label.
        label: String,
        /// Whether it is checked.
        checked: bool,
    },
    /// One radio button; a group is a set of them of which the component
    /// selects one.
    Radio {
        /// Its label.
        label: String,
        /// Whether it is the selected one.
        selected: bool,
    },
    /// An on/off switch. Hosts without a native switch show a check box.
    Toggle {
        /// Its label.
        label: String,
        /// Whether it is on.
        on: bool,
    },
    /// A value chosen by dragging along a track.
    Slider {
        /// The value.
        value: i64,
        /// The smallest value.
        min: i64,
        /// The largest value.
        max: i64,
    },
    /// Progress: a percentage, or `None` while how far is unknown.
    Progress {
        /// How far, 0 to 100.
        percent: Option<u8>,
    },
    /// One option chosen from a drop-down list.
    Select {
        /// The options.
        options: Vec<String>,
        /// The chosen one.
        selected: Option<usize>,
    },
    /// One item chosen from a visible list.
    ListBox {
        /// The items.
        items: Vec<String>,
        /// The chosen one.
        selected: Option<usize>,
    },
    /// A date.
    DatePicker {
        /// The date.
        date: CalendarDate,
    },
    /// A whole number, stepped up and down.
    Spinner {
        /// The value.
        value: i64,
        /// The smallest value.
        min: i64,
        /// The largest value.
        max: i64,
    },
    /// A horizontal rule between groups of content.
    Separator,
    /// Text that navigates when activated.
    Link {
        /// Its text.
        text: String,
    },
    /// Editable text of several lines.
    MultilineText {
        /// The text.
        value: String,
    },
    /// A picture.
    Image {
        /// The pixels.
        image: ImageData,
    },
}

#[allow(
    clippy::cast_precision_loss,
    reason = "control values are small integers; the float is only for assistive technology"
)]
fn float(value: i64) -> f32 {
    value as f32
}

impl Control {
    /// The accessibility role it has.
    #[must_use]
    pub fn role(&self) -> AccessibilityRole {
        match self {
            Self::Checkbox { .. } | Self::Toggle { .. } => AccessibilityRole::CheckBox,
            Self::Radio { .. } => AccessibilityRole::RadioButton,
            Self::Slider { .. } => AccessibilityRole::Slider,
            Self::Progress { .. } => AccessibilityRole::ProgressBar,
            Self::Select { .. } => AccessibilityRole::ComboBox,
            Self::ListBox { .. } => AccessibilityRole::List,
            Self::DatePicker { .. } | Self::MultilineText { .. } => AccessibilityRole::TextInput,
            Self::Spinner { .. } => AccessibilityRole::SpinButton,
            Self::Separator => AccessibilityRole::Separator,
            Self::Link { .. } => AccessibilityRole::Link,
            Self::Image { .. } => AccessibilityRole::Image,
        }
    }

    /// Whether it takes keyboard focus.
    #[must_use]
    pub fn focusable(&self) -> bool {
        !matches!(self, Self::Progress { .. } | Self::Separator | Self::Image { .. })
    }

    /// Its text as it reads — a label, a link's text, the chosen option, a
    /// date — or `None` when it has none.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        match self {
            Self::Checkbox { label, .. }
            | Self::Radio { label, .. }
            | Self::Toggle { label, .. } => Some(label.clone()),
            Self::Link { text } => Some(text.clone()),
            Self::MultilineText { value } => Some(value.clone()),
            Self::Select { options, selected } => {
                selected.and_then(|index| options.get(index).cloned())
            }
            Self::ListBox { items, selected } => {
                selected.and_then(|index| items.get(index).cloned())
            }
            Self::DatePicker { date } => Some(date.to_string()),
            Self::Slider { value, .. } | Self::Spinner { value, .. } => Some(value.to_string()),
            Self::Progress { percent } => percent.map(|percent| format!("{percent}%")),
            Self::Separator | Self::Image { .. } => None,
        }
    }

    /// The accessibility metadata a node of this control starts with: its
    /// role, focusability, name, and state.
    #[must_use]
    pub fn accessibility(&self) -> AccessibilityInfo {
        let info = AccessibilityInfo::new(self.role()).focusable(self.focusable());
        let checked = |on: bool| if on { CheckedState::Checked } else { CheckedState::Unchecked };
        match self {
            Self::Checkbox { label, checked: on } | Self::Toggle { label, on } => {
                info.name(label.clone()).checked(checked(*on))
            }
            Self::Radio { label, selected } => info.name(label.clone()).checked(checked(*selected)),
            Self::Slider { value, min, max } | Self::Spinner { value, min, max } => {
                info.range(float(*min), float(*max), float(*value), 1.0)
            }
            Self::Progress { percent: Some(percent) } => {
                info.range(0.0, 100.0, f32::from(*percent), 1.0)
            }
            Self::Progress { percent: None } => info.busy(true),
            Self::Select { .. } | Self::ListBox { .. } | Self::DatePicker { .. } => {
                match self.text() {
                    Some(text) => info.text_value(text),
                    None => info,
                }
            }
            Self::MultilineText { value } => info.text_value(value.clone()),
            Self::Link { text } => info.name(text.clone()),
            Self::Separator | Self::Image { .. } => info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_checked() {
        assert!(CalendarDate::new(2024, 2, 29).is_some());
        assert!(CalendarDate::new(2023, 2, 29).is_none());
        assert!(CalendarDate::new(2023, 13, 1).is_none());
        assert_eq!(
            CalendarDate::new(2026, 9, 5).map(|d| d.to_string()).as_deref(),
            Some("2026-09-05")
        );
    }

    #[test]
    fn a_control_describes_itself_to_assistive_technology() {
        let check = Control::Checkbox { label: "Remember me".into(), checked: true };
        let info = check.accessibility();
        assert_eq!(info.role(), AccessibilityRole::CheckBox);
        assert_eq!(info.name_hint(), Some("Remember me"));
        assert_eq!(info.checked_state(), Some(CheckedState::Checked));
        let select =
            Control::Select { options: vec!["Red".into(), "Blue".into()], selected: Some(1) };
        assert_eq!(select.text().as_deref(), Some("Blue"));
    }
}
