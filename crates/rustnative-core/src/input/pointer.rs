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
    pen: PenState,
    coalesced: Vec<PointerEvent>,
    predicted: Vec<PointerEvent>,
}

/// What a pen reports beyond position and pressure, in the form the hosts
/// that report most of it use (Apple Pencil, the browser's Pointer Events
/// level 3): each part is `None` where the device does not report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PenState {
    /// Angle from the surface, radians: 0 lying flat, π/2 upright.
    altitude: Option<Scalar>,
    /// Direction the pen leans, radians clockwise from the positive x axis.
    azimuth: Option<Scalar>,
    /// Rotation about the pen's own axis, radians.
    roll: Option<Scalar>,
    /// Distance above the surface while hovering, in logical pixels; `None`
    /// when touching or unknown.
    hover: Option<Scalar>,
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
            pen: PenState::default(),
            coalesced: Vec::new(),
            predicted: Vec::new(),
        }
    }

    /// The pen's tilt as altitude (radians from the surface, `0..=π/2`) and
    /// azimuth (radians clockwise from the positive x axis, `0..2π`).
    #[must_use]
    pub fn with_tilt(mut self, altitude: f32, azimuth: f32) -> Self {
        let tau = std::f32::consts::TAU;
        self.pen.altitude = Some(Scalar::new(altitude.clamp(0.0, std::f32::consts::FRAC_PI_2)));
        self.pen.azimuth = Some(Scalar::new(azimuth.rem_euclid(tau)));
        self
    }

    /// The pen's tilt from the pair Windows and the browser report: the
    /// plane angles, in degrees (`-90..=90`), between the pen and the
    /// surface's normal along x and along y — converted as the Pointer
    /// Events specification converts them.
    #[must_use]
    pub fn with_tilt_xy(self, tilt_x: f32, tilt_y: f32) -> Self {
        let (altitude, azimuth) = tilt_from_xy(tilt_x, tilt_y);
        self.with_tilt(altitude, azimuth)
    }

    /// The pen's rotation about its own axis, radians.
    #[must_use]
    pub fn with_roll(mut self, roll: f32) -> Self {
        self.pen.roll = Some(Scalar::new(roll.rem_euclid(std::f32::consts::TAU)));
        self
    }

    /// How far above the surface a hovering pen is, in logical pixels.
    #[must_use]
    pub fn with_hover_distance(mut self, distance: f32) -> Self {
        self.pen.hover = Some(Scalar::new(distance.max(0.0)));
        self
    }

    /// The samples the host merged into this one since the last delivered
    /// event, oldest first — for drawing a stroke at the device's rate.
    #[must_use]
    pub fn with_coalesced(mut self, samples: Vec<Self>) -> Self {
        self.coalesced = samples;
        self
    }

    /// The host's prediction of the next samples, for drawing ahead of the
    /// pen to hide latency; replaced, not kept, when the real ones arrive.
    #[must_use]
    pub fn with_predicted(mut self, samples: Vec<Self>) -> Self {
        self.predicted = samples;
        self
    }

    /// Altitude and azimuth, radians, if the device reports tilt.
    #[must_use]
    pub fn tilt(&self) -> Option<(f32, f32)> {
        Some((self.pen.altitude?.get(), self.pen.azimuth?.get()))
    }

    /// Rotation about the pen's axis, radians, if reported.
    #[must_use]
    pub fn roll(&self) -> Option<f32> {
        self.pen.roll.map(Scalar::get)
    }

    /// Hover distance in logical pixels, if hovering and reported.
    #[must_use]
    pub fn hover_distance(&self) -> Option<f32> {
        self.pen.hover.map(Scalar::get)
    }

    /// The merged samples behind this one, oldest first (empty when the
    /// host merged none or does not say).
    #[must_use]
    pub fn coalesced(&self) -> &[Self] {
        &self.coalesced
    }

    /// The predicted samples after this one.
    #[must_use]
    pub fn predicted(&self) -> &[Self] {
        &self.predicted
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

/// Tilt-X/tilt-Y in degrees to altitude and azimuth in radians (Pointer
/// Events level 3, "converting between tiltX/tiltY and altitudeAngle/
/// azimuthAngle").
fn tilt_from_xy(tilt_x: f32, tilt_y: f32) -> (f32, f32) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let (x, y) = (tilt_x.clamp(-90.0, 90.0), tilt_y.clamp(-90.0, 90.0));
    if x == 0.0 && y == 0.0 {
        return (FRAC_PI_2, 0.0);
    }
    // Clamped above, so "at least 90" is "exactly 90".
    if x.abs() >= 90.0 || y.abs() >= 90.0 {
        // Lying flat: the azimuth is the axis it lies along.
        let azimuth = y.to_radians().sin().atan2(x.to_radians().sin());
        return (0.0, azimuth.rem_euclid(TAU));
    }
    let (tan_x, tan_y) = (x.to_radians().tan(), y.to_radians().tan());
    let azimuth = tan_y.atan2(tan_x).rem_euclid(TAU);
    let altitude = (1.0 / tan_x.hypot(tan_y)).atan();
    (altitude, azimuth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pens_tilt_pair_converts_to_altitude_and_azimuth() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let pen = |x: f32, y: f32| {
            PointerEvent::new(1, PointerKind::Pen, Point::new(0, 0), Duration::ZERO)
                .with_tilt_xy(x, y)
                .tilt()
                .unwrap()
        };
        let (altitude, _) = pen(0.0, 0.0);
        assert!(close(altitude, FRAC_PI_2), "upright");
        let (altitude, azimuth) = pen(45.0, 0.0);
        assert!(close(altitude, FRAC_PI_4) && close(azimuth, 0.0), "leaning toward +x");
        let (altitude, azimuth) = pen(0.0, -45.0);
        assert!(close(altitude, FRAC_PI_4) && close(azimuth, 3.0 * FRAC_PI_2), "toward -y");
        let (altitude, azimuth) = pen(-90.0, 0.0);
        assert!(close(altitude, 0.0) && close(azimuth, PI), "flat along -x");
    }

    #[test]
    fn coalesced_and_predicted_samples_ride_along() {
        let at = |x| PointerEvent::new(1, PointerKind::Pen, Point::new(x, 0), Duration::ZERO);
        let sample = at(3)
            .with_coalesced(vec![at(1), at(2)])
            .with_predicted(vec![at(4)])
            .with_roll(7.0)
            .with_hover_distance(-2.0);
        assert_eq!(sample.coalesced().len(), 2);
        assert_eq!(sample.predicted()[0].position(), Point::new(4, 0));
        assert!(sample.roll().is_some_and(|roll| roll < std::f32::consts::TAU));
        assert_eq!(sample.hover_distance(), Some(0.0));
        assert_eq!(at(0).tilt(), None, "a mouse reports no tilt");
    }

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
