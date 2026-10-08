//! The numbers Rust and the Java host library agree on — the twin of
//! `java/dev/rustnative/android/Rn.java`, held in step by the test below.

#![allow(dead_code, reason = "both halves of the protocol are listed, used or not on this side")]

// View kinds (`RnViews.create`).
pub(crate) const LABEL: i32 = 1;
pub(crate) const BUTTON: i32 = 2;
pub(crate) const TEXT_INPUT: i32 = 3;
pub(crate) const PASSWORD: i32 = 4;
pub(crate) const CONTAINER: i32 = 5;
pub(crate) const SCROLL: i32 = 6;
pub(crate) const CANVAS: i32 = 7;
pub(crate) const SURFACE: i32 = 8;
pub(crate) const TAB_BAR: i32 = 9;
pub(crate) const CHECKBOX: i32 = 11;
pub(crate) const RADIO: i32 = 12;
pub(crate) const TOGGLE: i32 = 13;
pub(crate) const SLIDER: i32 = 14;
pub(crate) const PROGRESS: i32 = 15;
pub(crate) const SELECT: i32 = 16;
pub(crate) const LIST_BOX: i32 = 17;
pub(crate) const DATE: i32 = 18;
pub(crate) const SPINNER: i32 = 19;
pub(crate) const SEPARATOR: i32 = 20;
pub(crate) const LINK: i32 = 21;
pub(crate) const MULTILINE: i32 = 22;
pub(crate) const IMAGE: i32 = 23;
pub(crate) const FOREIGN: i32 = 24;
pub(crate) const WEB: i32 = 25;
pub(crate) const MEDIA: i32 = 26;
pub(crate) const CAMERA: i32 = 27;

// View events (`RnBridge.nativeViewEvent`).
pub(crate) const EV_CLICK: i32 = 1;
pub(crate) const EV_TEXT: i32 = 2;
pub(crate) const EV_TOGGLED: i32 = 3;
pub(crate) const EV_VALUE: i32 = 4;
pub(crate) const EV_SELECTION: i32 = 5;
pub(crate) const EV_DATE: i32 = 6;
pub(crate) const EV_FOCUS: i32 = 7;
pub(crate) const EV_BLUR: i32 = 8;
pub(crate) const EV_SCROLL: i32 = 9;
pub(crate) const EV_TAB: i32 = 10;
pub(crate) const EV_SURFACE: i32 = 11;
pub(crate) const EV_PAGE: i32 = 12;
pub(crate) const EV_ACCESSIBILITY: i32 = 13;
pub(crate) const EV_DRAG: i32 = 14;

// Lifecycle (`RnBridge.nativeLifecycle`).
pub(crate) const LC_START: i32 = 1;
pub(crate) const LC_RESUME: i32 = 2;
pub(crate) const LC_PAUSE: i32 = 3;
pub(crate) const LC_STOP: i32 = 4;
pub(crate) const LC_DESTROY: i32 = 5;
pub(crate) const LC_DESTROY_FINISHING: i32 = 6;
pub(crate) const LC_SAVE: i32 = 7;
pub(crate) const LC_CONFIGURATION: i32 = 8;
pub(crate) const LC_TRIM: i32 = 9;
pub(crate) const LC_LOW_MEMORY: i32 = 10;
pub(crate) const LC_MULTI_WINDOW: i32 = 11;
pub(crate) const LC_FOCUS: i32 = 12;
pub(crate) const LC_POSTURE: i32 = 13;

// Back (`RnBridge.nativeBack`).
pub(crate) const BACK_STARTED: i32 = 1;
pub(crate) const BACK_PROGRESSED: i32 = 2;
pub(crate) const BACK_CANCELLED: i32 = 3;
pub(crate) const BACK_INVOKED: i32 = 4;

// Style (`RnStyle.apply`).
pub(crate) const ST_BACKGROUND: i32 = 1;
pub(crate) const ST_FOREGROUND: i32 = 2;
pub(crate) const ST_BORDER: i32 = 4;
pub(crate) const ST_RADIUS: i32 = 8;
pub(crate) const ST_ELEVATION: i32 = 16;
pub(crate) const STATE_NORMAL: usize = 0;
pub(crate) const STATE_HOVERED: usize = 1;
pub(crate) const STATE_FOCUSED: usize = 2;
pub(crate) const STATE_PRESSED: usize = 3;
pub(crate) const STATE_DISABLED: usize = 4;
pub(crate) const STATES: usize = 5;
pub(crate) const STATE_INTS: usize = 4;
pub(crate) const STATE_FLOATS: usize = 2;

/// The tag menu choices are reported with.
pub(crate) const MENU_TAG: i32 = -1;

#[cfg(test)]
mod tests {
    /// Every `static final int NAME = value;` in `Rn.java`.
    fn java_constants() -> Vec<(String, i64)> {
        include_str!("../java/dev/rustnative/android/Rn.java")
            .lines()
            .filter_map(|line| {
                let rest = line.trim().strip_prefix("static final int ")?;
                let (name, value) = rest.split_once('=')?;
                let value = value.trim().trim_end_matches(';').trim().parse().ok()?;
                Some((name.trim().to_owned(), value))
            })
            .collect()
    }

    #[test]
    fn the_java_constants_are_the_rust_constants() {
        let rust: &[(&str, i64)] = &[
            ("LABEL", super::LABEL.into()),
            ("BUTTON", super::BUTTON.into()),
            ("TEXT_INPUT", super::TEXT_INPUT.into()),
            ("PASSWORD", super::PASSWORD.into()),
            ("CONTAINER", super::CONTAINER.into()),
            ("SCROLL", super::SCROLL.into()),
            ("CANVAS", super::CANVAS.into()),
            ("SURFACE", super::SURFACE.into()),
            ("TAB_BAR", super::TAB_BAR.into()),
            ("CHECKBOX", super::CHECKBOX.into()),
            ("RADIO", super::RADIO.into()),
            ("TOGGLE", super::TOGGLE.into()),
            ("SLIDER", super::SLIDER.into()),
            ("PROGRESS", super::PROGRESS.into()),
            ("SELECT", super::SELECT.into()),
            ("LIST_BOX", super::LIST_BOX.into()),
            ("DATE", super::DATE.into()),
            ("SPINNER", super::SPINNER.into()),
            ("SEPARATOR", super::SEPARATOR.into()),
            ("LINK", super::LINK.into()),
            ("MULTILINE", super::MULTILINE.into()),
            ("IMAGE", super::IMAGE.into()),
            ("FOREIGN", super::FOREIGN.into()),
            ("WEB", super::WEB.into()),
            ("MEDIA", super::MEDIA.into()),
            ("CAMERA", super::CAMERA.into()),
            ("EV_CLICK", super::EV_CLICK.into()),
            ("EV_TEXT", super::EV_TEXT.into()),
            ("EV_TOGGLED", super::EV_TOGGLED.into()),
            ("EV_VALUE", super::EV_VALUE.into()),
            ("EV_SELECTION", super::EV_SELECTION.into()),
            ("EV_DATE", super::EV_DATE.into()),
            ("EV_FOCUS", super::EV_FOCUS.into()),
            ("EV_BLUR", super::EV_BLUR.into()),
            ("EV_SCROLL", super::EV_SCROLL.into()),
            ("EV_TAB", super::EV_TAB.into()),
            ("EV_SURFACE", super::EV_SURFACE.into()),
            ("EV_PAGE", super::EV_PAGE.into()),
            ("EV_ACCESSIBILITY", super::EV_ACCESSIBILITY.into()),
            ("EV_DRAG", super::EV_DRAG.into()),
            ("LC_START", super::LC_START.into()),
            ("LC_RESUME", super::LC_RESUME.into()),
            ("LC_PAUSE", super::LC_PAUSE.into()),
            ("LC_STOP", super::LC_STOP.into()),
            ("LC_DESTROY", super::LC_DESTROY.into()),
            ("LC_DESTROY_FINISHING", super::LC_DESTROY_FINISHING.into()),
            ("LC_SAVE", super::LC_SAVE.into()),
            ("LC_CONFIGURATION", super::LC_CONFIGURATION.into()),
            ("LC_TRIM", super::LC_TRIM.into()),
            ("LC_LOW_MEMORY", super::LC_LOW_MEMORY.into()),
            ("LC_MULTI_WINDOW", super::LC_MULTI_WINDOW.into()),
            ("LC_FOCUS", super::LC_FOCUS.into()),
            ("LC_POSTURE", super::LC_POSTURE.into()),
            ("BACK_STARTED", super::BACK_STARTED.into()),
            ("BACK_PROGRESSED", super::BACK_PROGRESSED.into()),
            ("BACK_CANCELLED", super::BACK_CANCELLED.into()),
            ("BACK_INVOKED", super::BACK_INVOKED.into()),
            ("ST_BACKGROUND", super::ST_BACKGROUND.into()),
            ("ST_FOREGROUND", super::ST_FOREGROUND.into()),
            ("ST_BORDER", super::ST_BORDER.into()),
            ("ST_RADIUS", super::ST_RADIUS.into()),
            ("ST_ELEVATION", super::ST_ELEVATION.into()),
            ("STATE_NORMAL", 0),
            ("STATE_HOVERED", 1),
            ("STATE_FOCUSED", 2),
            ("STATE_PRESSED", 3),
            ("STATE_DISABLED", 4),
            ("STATES", 5),
            ("STATE_INTS", 4),
            ("STATE_FLOATS", 2),
        ];
        let java = java_constants();
        assert_eq!(java.len(), rust.len(), "Rn.java and protocol.rs list different constants");
        for (name, value) in rust {
            let found = java.iter().find(|(java_name, _)| java_name == name);
            assert_eq!(found.map(|(_, value)| *value), Some(*value), "{name}");
        }
        assert_eq!(super::STATE_DISABLED + 1, super::STATES);
        assert_eq!(super::STATE_NORMAL, 0);
        assert_eq!(super::STATE_HOVERED, 1);
        assert_eq!(super::STATE_FOCUSED, 2);
        assert_eq!(super::STATE_PRESSED, 3);
        assert_eq!(super::STATE_INTS, 4);
        assert_eq!(super::STATE_FLOATS, 2);
    }
}
