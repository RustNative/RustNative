//! Safe areas, cutouts, hinges, and split-screen as layout-model properties
//! (`PLAN.md` Milestone 39, and Milestone 35's "handled as layout-model
//! properties").
//!
//! The activity draws edge to edge; the window's insets — system bars,
//! display cutout, and the soft keyboard while it shows — become
//! `keys::SAFE_AREA` in dp, logical (start/end follow the layout
//! direction). Split-screen becomes `keys::WINDOW_MODE`; a foldable's hinge
//! becomes `keys::POSTURE` (`posture`).

use rustnative_core::{
    EdgeInsets, LayoutDirection, PointerPrecision, Scalar, WindowId, WindowMode, keys,
};

use crate::units::to_dp;

/// The safe area from a window's insets (pixels: bars, cutout, IME, and
/// gestures, each left, top, right, bottom): the larger of the bars, the
/// cutout, and the keyboard on each edge, in dp, start and end by
/// direction. System gesture insets are not part of it — content may sit
/// under them; only edge swipes must avoid them (`input::gestures`).
pub(crate) fn safe_area(
    insets: &[i32; 16],
    density: f32,
    direction: LayoutDirection,
) -> EdgeInsets {
    let edge = |offset: usize| {
        let pixels = insets[offset].max(insets[4 + offset]).max(insets[8 + offset]);
        i32::try_from(to_dp(pixels, density)).unwrap_or(i32::MAX)
    };
    let (left, top, right, bottom) = (edge(0), edge(1), edge(2), edge(3));
    let (start, end) =
        if direction == LayoutDirection::Rtl { (right, left) } else { (left, right) };
    EdgeInsets::logical(top, end, bottom, start)
}

/// The window mode: the whole display, or a share of it.
pub(crate) fn window_mode(multi_window: bool, window_width: i32, display_width: i32) -> WindowMode {
    if !multi_window || display_width <= 0 {
        return WindowMode::Full;
    }
    #[allow(clippy::cast_precision_loss, reason = "pixel widths, far inside f32's exact range")]
    let fraction = (window_width as f32 / display_width as f32).clamp(0.0, 1.0);
    if fraction >= 0.99 {
        WindowMode::Full
    } else {
        WindowMode::Split { fraction: Scalar::new(fraction) }
    }
}

/// Feeds `window`'s environment values from its activity: safe area, window
/// mode and width, posture, pointer precision.
#[cfg(target_os = "android")]
pub(crate) fn window_changed(registry: &mut crate::registry::WindowRegistry, window: WindowId) {
    let Some(runtime) = registry.windows.get(&window) else { return };
    let direction = registry.with_application(|application| application.layout_direction(window));
    let area = safe_area(&runtime.insets, runtime.density, direction);
    let width = runtime.size.width;
    let display = crate::posture::display_width(runtime);
    let mode = window_mode(registry.traits.multi_window, runtime.pixels.0, display);
    let posture = crate::posture::read(runtime);
    let precision =
        if runtime.input.fine_pointer { PointerPrecision::Fine } else { PointerPrecision::Coarse };
    if let Some(renderer) =
        registry.windows.get_mut(&window).and_then(|runtime| runtime.renderer.as_mut())
    {
        renderer.safe_area = area;
    }
    registry.with_application(|application| {
        application.set_window_environment(window, &keys::SAFE_AREA, area);
        application.set_window_environment(window, &keys::WINDOW_MODE, mode);
        application.set_window_environment(window, &keys::WINDOW_WIDTH, width);
        application.set_window_environment(window, &keys::POSTURE, posture);
        application.set_window_environment(window, &keys::POINTER, precision);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_safe_area_is_the_largest_inset_per_edge_in_dp() {
        // Status bar 96 px, a 120 px cutout at the top, navigation 66 px at
        // the bottom, the keyboard 800 px; density 2.75.
        let mut insets = [0; 16];
        insets[1] = 96;
        insets[3] = 66;
        insets[5] = 120;
        insets[11] = 800;
        insets[12] = 30; // a gesture inset: not part of it
        let area = safe_area(&insets, 2.75, LayoutDirection::Ltr);
        assert_eq!(area, EdgeInsets::logical(44, 0, 291, 0));
        insets[0] = 55; // a landscape navigation bar on the left
        let ltr = safe_area(&insets, 2.75, LayoutDirection::Ltr);
        let rtl = safe_area(&insets, 2.75, LayoutDirection::Rtl);
        assert_eq!(ltr.start, 20);
        assert_eq!(rtl.end, 20);
    }

    #[test]
    fn split_screen_is_a_share_of_the_display() {
        assert_eq!(window_mode(false, 540, 1080), WindowMode::Full);
        assert_eq!(window_mode(true, 540, 1080), WindowMode::Split { fraction: Scalar::new(0.5) });
        assert_eq!(window_mode(true, 1080, 1080), WindowMode::Full);
    }
}
