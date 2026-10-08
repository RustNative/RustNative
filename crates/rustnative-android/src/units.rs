//! Logical pixels (dp) and device pixels: the unit mapping
//! (`rustnative_style::ANDROID_UNITS`).

/// Device pixels to dp, rounded up.
pub(crate) fn to_dp(pixels: i32, density: f32) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a view's size in pixels, small and non-negative"
    )]
    let dp = (pixels.max(0) as f32 / density.max(0.1)).ceil() as u32;
    dp
}

/// dp to device pixels, rounded half away from zero.
pub(crate) fn to_px(dp: i32, density: f32) -> i32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "a length in dp times a density, well inside i32"
    )]
    let px = (dp as f32 * density).round() as i32;
    px
}

#[cfg(test)]
mod tests {
    #[test]
    fn units_round_the_right_way() {
        assert_eq!(super::to_dp(105, 2.75), 39); // 38.2 rounds up
        assert_eq!(super::to_px(10, 2.75), 28); // 27.5 rounds half away from zero
        assert_eq!(super::to_px(-10, 2.75), -28);
    }
}
