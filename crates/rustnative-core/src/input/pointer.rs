//! Pointer (mouse, touch, pen) and wheel payloads.

use std::time::Duration;

use super::Scalar;
use crate::event::KeyModifiers;
use crate::layout::Point;

/// The physical device a pointer event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerKind {
    /// A mouse or other indirect pointing device (including a trackpad
    /// driving the cursor).
    Mouse,
    /// A finger on a touch screen.
    Touch,
    /// A pen or stylus.
    Pen,
}

/// A pointer button. Touch contacts and pen tips report [`Self::Primary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// Left mouse button, touch contact, or pen tip.
    Primary,
    /// Right mouse button or pen barrel button.
    Secondary,
    /// Middle mouse button.
    Middle,
    /// The "back" extra mouse button.
    Back,
    /// The "forward" extra mouse button.
    Forward,
}

impl PointerButton {
    const fn bit(self) -> u8 {
        match self {
            Self::Primary => 1,
            Self::Secondary => 1 << 1,
            Self::Middle => 1 << 2,
            Self::Back => 1 << 3,
            Self::Forward => 1 << 4,
        }
    }
}

/// The set of pointer buttons held down at the time of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PointerButtons(u8);

impl PointerButtons {
    /// No buttons held.
    #[must_use]
    pub const fn none() -> Self {
        Self(0)
    }

    /// `self` with `button` added.
    #[must_use]
    pub const fn with(self, button: PointerButton) -> Self {
        Self(self.0 | button.bit())
    }

    /// Whether `button` is held.
    #[must_use]
    pub const fn contains(self, button: PointerButton) -> bool {
        self.0 & button.bit() != 0
    }

    /// Whether no button is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// One pointer sample, delivered in the target node's local coordinate
/// space.
///
/// Constructed by a backend through [`Self::new`] and the `with_*`
/// builders rather than a struct literal, so fields can be added in later
/// milestones (M29 adds a canvas hit-region id) without breaking backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerEvent {
    pointer_id: u32,
    kind: PointerKind,
    position: Point,
    button: Option<PointerButton>,
    buttons: PointerButtons,
    modifiers: KeyModifiers,
    pressure: Option<Scalar>,
    timestamp: Duration,
    region: Option<u32>,
}

impl PointerEvent {
    /// A sample for pointer `pointer_id` at `position` (target-local),
    /// taken at `timestamp` on the backend's monotonic input clock.
    ///
    /// `pointer_id` distinguishes simultaneous contacts (multi-touch); the
    /// mouse is conventionally `0`.
    #[must_use]
    pub fn new(pointer_id: u32, kind: PointerKind, position: Point, timestamp: Duration) -> Self {
        Self {
            pointer_id,
            kind,
            position,
            button: None,
            buttons: PointerButtons::none(),
            modifiers: KeyModifiers::default(),
            pressure: None,
            timestamp,
            region: None,
        }
    }

    /// The canvas hit region this sample landed in (see
    /// [`crate::DrawList::hit_region`]); set by a backend for input on a
    /// canvas.
    #[must_use]
    pub fn with_region(mut self, region: Option<u32>) -> Self {
        self.region = region;
        self
    }

    /// Which hit region of a canvas this sample landed in, if the target is
    /// a canvas and the point is inside one of its declared regions.
    #[must_use]
    pub const fn region(&self) -> Option<u32> {
        self.region
    }

    /// The button whose state changed in this event (down/up only).
    #[must_use]
    pub fn with_button(mut self, button: PointerButton) -> Self {
        self.button = Some(button);
        self
    }

    /// Every button held at the time of the event.
    #[must_use]
    pub fn with_buttons(mut self, buttons: PointerButtons) -> Self {
        self.buttons = buttons;
        self
    }

    /// Keyboard modifiers held at the time of the event.
    #[must_use]
    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Normalized pressure in `0.0..=1.0`, for devices that report it.
    #[must_use]
    pub fn with_pressure(mut self, pressure: f32) -> Self {
        self.pressure = Some(Scalar::new(pressure.clamp(0.0, 1.0)));
        self
    }

    /// Returns `self` re-expressed at `position`, e.g. after a backend
    /// re-targets the sample to an ancestor's coordinate space.
    #[must_use]
    pub fn at(mut self, position: Point) -> Self {
        self.position = position;
        self
    }

    /// Which contact this is.
    #[must_use]
    pub const fn pointer_id(&self) -> u32 {
        self.pointer_id
    }

    /// The device kind.
    #[must_use]
    pub const fn kind(&self) -> PointerKind {
        self.kind
    }

    /// Position in the target node's local coordinates.
    #[must_use]
    pub const fn position(&self) -> Point {
        self.position
    }

    /// The button that changed, for down/up events.
    #[must_use]
    pub const fn button(&self) -> Option<PointerButton> {
        self.button
    }

    /// Buttons held.
    #[must_use]
    pub const fn buttons(&self) -> PointerButtons {
        self.buttons
    }

    /// Modifiers held.
    #[must_use]
    pub const fn modifiers(&self) -> KeyModifiers {
        self.modifiers
    }

    /// Pressure, if the device reports it.
    #[must_use]
    pub fn pressure(&self) -> Option<f32> {
        self.pressure.map(Scalar::get)
    }

    /// When the sample was taken, on the backend's monotonic input clock.
    #[must_use]
    pub const fn timestamp(&self) -> Duration {
        self.timestamp
    }
}

/// A scroll-wheel or trackpad scroll amount.
///
/// Positive `y` scrolls content **down** (the wheel rotated toward the
/// user), positive `x` scrolls content **right** — the direction a reader
/// moves through a document, independent of any platform's sign
/// convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelDelta {
    /// Discrete wheel notches, in units of 1/[`WheelDelta::DETENT`] of a
    /// notch so that high-resolution wheels are not rounded away.
    Lines {
        /// Horizontal amount.
        x: i32,
        /// Vertical amount.
        y: i32,
    },
    /// A precise, pixel-granular scroll (a precision touchpad or a
    /// free-spinning high-resolution wheel), in logical pixels.
    Pixels {
        /// Horizontal amount.
        x: i32,
        /// Vertical amount.
        y: i32,
    },
}

impl WheelDelta {
    /// Sub-units per wheel notch for [`Self::Lines`]. The same resolution
    /// every major platform reports (Windows' `WHEEL_DELTA`, the Web's
    /// `wheelDelta`), so no platform's precision is lost.
    pub const DETENT: i32 = 120;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_are_a_set() {
        let buttons = PointerButtons::none().with(PointerButton::Primary).with(PointerButton::Back);
        assert!(buttons.contains(PointerButton::Primary));
        assert!(buttons.contains(PointerButton::Back));
        assert!(!buttons.contains(PointerButton::Secondary));
        assert!(PointerButtons::none().is_empty());
    }

    #[test]
    fn pressure_is_clamped_into_the_unit_interval() {
        let sample = PointerEvent::new(1, PointerKind::Pen, Point::new(0, 0), Duration::ZERO)
            .with_pressure(3.0);
        assert_eq!(sample.pressure(), Some(1.0));
        let sample = sample.with_pressure(-1.0);
        assert_eq!(sample.pressure(), Some(0.0));
    }
}
