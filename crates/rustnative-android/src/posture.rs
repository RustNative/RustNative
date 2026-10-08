//! A foldable's hinge as `keys::POSTURE` (`PLAN.md` Milestones 35 and 39),
//! and the display's size for the split-screen share.
//!
//! The hinge comes from the window extensions in the device's system image
//! (`RnPosture`): a half-opened fold is `Posture::HalfOpened`; a flat fold
//! with a physical hinge that still divides the screen is
//! `Posture::Separated`; anything else is `Posture::Flat`. The hinge is in
//! the window's coordinates, in dp.

use rustnative_core::{Posture, Rect};

// `FoldingFeature` types and states.
const TYPE_HINGE: i32 = 2;
const STATE_FLAT: i32 = 1;
const STATE_HALF_OPENED: i32 = 2;

/// The posture the host library's numbers describe (six per feature: type,
/// state, left, top, right, bottom in pixels).
pub(crate) fn from_features(features: &[i32], density: f32) -> Posture {
    let dp = |pixels: i32| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "a hinge's edge in pixels over the density"
        )]
        let value = (pixels as f32 / density.max(0.1)).round() as i32;
        value
    };
    for feature in features.chunks_exact(6) {
        let (kind, state) = (feature[0], feature[1]);
        let hinge = Rect::new(
            dp(feature[2]),
            dp(feature[3]),
            dp(feature[4] - feature[2]),
            dp(feature[5] - feature[3]),
        );
        if state == STATE_HALF_OPENED {
            return Posture::HalfOpened { hinge };
        }
        if state == STATE_FLAT && (kind == TYPE_HINGE || hinge.width > 0 && hinge.height > 0) {
            return Posture::Separated { hinge };
        }
    }
    Posture::Flat
}

#[cfg(target_os = "android")]
pub(crate) use platform::{display_width, read, watch};

#[cfg(target_os = "android")]
mod platform {
    use rustnative_core::Posture;

    use crate::jni_host::{Arg, Class, call_static};
    use crate::registry::WindowRuntime;

    /// Starts following the activity's folding features.
    pub(crate) fn watch(runtime: &WindowRuntime) {
        if let Some(activity) = &runtime.activity {
            let window = i64::try_from(runtime.id.get()).unwrap_or(0);
            let _ = call_static(
                Class::Posture,
                "watch",
                "(Landroid/app/Activity;J)V",
                &[Arg::Obj(activity), Arg::Long(window)],
            );
        }
    }

    /// The display's width in pixels (for the split-screen share).
    pub(crate) fn display_width(runtime: &WindowRuntime) -> i32 {
        runtime
            .activity
            .as_ref()
            .and_then(|activity| {
                call_static(
                    Class::Posture,
                    "display",
                    "(Landroid/app/Activity;)[I",
                    &[Arg::Obj(activity)],
                )
                .ok()
            })
            .and_then(|display| display.ints().first().copied())
            .unwrap_or(runtime.pixels.0)
    }

    /// The window's posture, as last reported.
    pub(crate) fn read(runtime: &WindowRuntime) -> Posture {
        let Some(activity) = &runtime.activity else { return Posture::Flat };
        let features = call_static(
            Class::Posture,
            "features",
            "(Landroid/app/Activity;)[I",
            &[Arg::Obj(activity)],
        )
        .map(crate::jni_host::Ret::ints)
        .unwrap_or_default();
        super::from_features(&features, runtime.density)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_features_become_postures() {
        assert_eq!(from_features(&[], 2.0), Posture::Flat);
        // A half-opened fold across the middle of a 2208 × 1840 px window.
        let half = from_features(&[1, STATE_HALF_OPENED, 0, 910, 2208, 930], 2.0);
        assert_eq!(half, Posture::HalfOpened { hinge: Rect::new(0, 455, 1104, 10) });
        // A flat device with a physical hinge still divides.
        let hinge = from_features(&[TYPE_HINGE, STATE_FLAT, 1350, 0, 1434, 1800], 2.0);
        assert!(matches!(hinge, Posture::Separated { .. }));
        // A flat fold with no width occludes and divides nothing.
        assert_eq!(from_features(&[1, STATE_FLAT, 0, 920, 2208, 920], 2.0), Posture::Flat);
    }
}
