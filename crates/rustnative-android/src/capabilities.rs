//! What this backend realizes on the running device — answered from the
//! device (`RnPlatform.device`), never from the build (`PLAN.md` 2.5).

use rustnative_core::Capability;

/// What the device has.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is an independent fact the device answers yes or no"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DeviceFacts {
    pub(crate) api_level: i32,
    pub(crate) keyboard: bool,
    pub(crate) stylus: bool,
    pub(crate) mouse: bool,
    pub(crate) gamepad: bool,
    pub(crate) touch: bool,
    pub(crate) camera: bool,
    pub(crate) play_services: bool,
    pub(crate) web_view: bool,
    pub(crate) printing: bool,
    pub(crate) camera_allowed: bool,
}

impl DeviceFacts {
    /// The facts from `RnPlatform.device`'s numbers.
    pub(crate) fn from_values(values: &[i32]) -> Self {
        let flag = |index: usize| values.get(index).is_some_and(|value| *value != 0);
        Self {
            api_level: values.first().copied().unwrap_or(0),
            keyboard: flag(1),
            stylus: flag(2),
            mouse: flag(3),
            gamepad: flag(4),
            touch: flag(5),
            camera: flag(6),
            play_services: flag(7),
            web_view: flag(8),
            printing: flag(9),
            camera_allowed: flag(10),
        }
    }
}

/// Every capability realized on a device with `facts`.
pub(crate) fn realized(facts: &DeviceFacts) -> Vec<Capability> {
    let mut capabilities = vec![
        // Each window is an activity of its own (`registry::sync`).
        Capability::MultipleWindows,
        // The options menu (`menus`).
        Capability::Menus,
        // The layout engine mirrors placement; views mirror what they draw.
        Capability::RightToLeft,
        // Night mode, font scale, contrast, animations, locale
        // (`host_traits`), followed through configuration changes.
        Capability::HostTraits,
        Capability::SystemAppearance,
        Capability::ReducedMotionPreference,
        // The activity lifecycle (`lifecycle`).
        Capability::Lifecycle,
        // `ACTION_VIEW` intents for the application's schemes (`intents`).
        Capability::DeepLinks,
        // `Choreographer`-paced transitions and animations (`animation`).
        Capability::Animations,
        // Canvas nodes replayed with `Canvas` and `StaticLayout` (`canvas`).
        Capability::CustomDrawing,
        // `SurfaceView` surfaces as `ANativeWindow`s (`surface`).
        Capability::NativeSurfaces,
        // `VideoView` with the host's controls (`foreign`).
        Capability::MediaPlayback,
    ];
    if facts.web_view {
        capabilities.push(Capability::WebContent);
    }
    if facts.camera_allowed {
        // A Camera2 preview, once the person allowed the camera.
        capabilities.push(Capability::Camera);
    }
    if facts.touch {
        capabilities.push(Capability::Touch);
    }
    if facts.stylus {
        capabilities.push(Capability::Pen);
    }
    if facts.mouse || facts.stylus {
        // A hovering mouse or pen (`ACTION_HOVER_*`).
        capabilities.push(Capability::Hover);
    }
    if facts.keyboard {
        // Shortcuts need keys to press.
        capabilities.push(Capability::CommandShortcuts);
    }
    capabilities
}

/// What the running device realizes.
#[cfg(target_os = "android")]
pub(crate) fn current() -> rustnative_core::PlatformCapabilities {
    use crate::jni_host::{Arg, Class, call_static};
    let Some(context) = crate::entry::application_context() else {
        return rustnative_core::PlatformCapabilities::default();
    };
    let values = call_static(
        Class::Platform,
        "device",
        "(Landroid/content/Context;)[I",
        &[Arg::Obj(&context)],
    )
    .map(crate::jni_host::Ret::ints)
    .unwrap_or_default();
    rustnative_core::PlatformCapabilities::new(realized(&DeviceFacts::from_values(&values)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_follow_the_device() {
        let phone = DeviceFacts::from_values(&[36, 0, 0, 0, 0, 1, 1, 1, 1, 1]);
        let answered = realized(&phone);
        assert!(answered.contains(&Capability::Touch));
        assert!(!answered.contains(&Capability::Pen), "no stylus on this phone");
        assert!(!answered.contains(&Capability::Hover), "a finger does not hover");
        assert!(!answered.contains(&Capability::CommandShortcuts), "no keyboard to press them on");
        // Never claimed on Android: the host places and sizes every window.
        assert!(!answered.contains(&Capability::WindowManagement));
        assert!(!answered.contains(&Capability::WindowPlacement));
        let tablet = DeviceFacts::from_values(&[35, 1, 1, 1, 0, 1, 1, 1, 1, 1]);
        let answered = realized(&tablet);
        assert!(answered.contains(&Capability::Pen));
        assert!(answered.contains(&Capability::Hover));
        assert!(answered.contains(&Capability::CommandShortcuts));
    }
}
