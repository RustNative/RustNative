//! Styling, host traits, direction, and the environment on a real device
//! (Phase 2): the `ANDROID` table is what the backend applies; a leaf's
//! padding insets its text and widens its box; an unstyled
//! control keeps the device theme's look; night mode and font scale reach
//! the environment and restyle the same views; right to left mirrors with
//! the same views; the safe area is the window's insets.

use std::time::Duration;

use rustnative_core::{
    Application, Color, ColorScheme, Component, EdgeInsets, Event, LayoutDirection, Locale, Node,
    Scalar, Size, Typography, VisualStyle, Window, WindowId, keys,
};

use super::harness::{Harness, Instrumentation, java, keep, on_main, with_kept};
use crate::jni_host::{Arg, Class};

/// A styled button beside a plain one, in a row, and a padded label beside
/// a plain one.
struct Styled;

impl Component for Styled {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let style = VisualStyle::default()
            .background(Color::rgb(200, 30, 40))
            .foreground(Color::rgb(10, 20, 250))
            .border(Color::rgb(0, 0, 0))
            .border_radius(8)
            .typography(Typography { family: "serif".into(), size: 20, weight: 700 });
        Node::column(
            "root",
            [
                Node::row(
                    "row",
                    [
                        Node::button("styled", "Styled").with_style(style),
                        Node::button("plain", "Plain"),
                    ],
                ),
                Node::label("caption", "Caption"),
                Node::row(
                    "badges",
                    [
                        Node::label("badge", "Badge"),
                        Node::label("padded", "Badge").with_style(
                            VisualStyle::default().padding(EdgeInsets {
                                top: 2,
                                end: 4,
                                bottom: 2,
                                start: 12,
                            }),
                        ),
                    ],
                ),
            ],
        )
    }
    fn update(&mut self, _event: Event) {}
}

fn style_of(harness: &Harness, key: &str) -> Vec<f32> {
    let view = harness.expect(key);
    java(Class::Probe, "style", "(Landroid/view/View;)[F", &[Arg::Obj(&view)]).floats()
}

fn color(bits: f32) -> u32 {
    bits.to_bits()
}

pub(super) fn the_android_table_is_what_the_backend_applies(_: &Instrumentation) {
    on_main(|| {
        let harness =
            Harness::launch(Application::new(Styled, Window::new("Styled", Size::new(360, 400))));
        let density = harness.density();
        let style = style_of(&harness, "styled");
        assert_eq!(color(style[0]), 0xffc8_1e28, "background red");
        assert_eq!(color(style[1]), 0xff0a_14fa, "text blue");
        let text_scale = harness.with_application(|application| {
            application.environment_for(WindowId::PRIMARY, &keys::TEXT_SCALE).get()
        });
        assert!(
            (style[2] - crate::styling::font_pixels(20, text_scale, density)).abs() < 0.5,
            "font size {}",
            style[2]
        );
        assert!((style[3] - 700.0).abs() < 0.5, "font weight {}", style[3]);
        assert!((style[4] - 8.0 * density).abs() < 0.5, "corner radius {}", style[4]);
        // An unstyled button keeps the theme's own background.
        let plain = harness.expect("plain");
        let plain_background = java(
            Class::Probe,
            "backgroundClass",
            "(Landroid/view/View;)Ljava/lang/String;",
            &[Arg::Obj(&plain)],
        )
        .string();
        assert_ne!(
            plain_background.as_deref(),
            Some("android.graphics.drawable.StateListDrawable"),
            "the plain button is not the framework's box"
        );
        assert_eq!(
            color(style_of(&harness, "plain")[0]),
            0,
            "no framework box behind the plain button"
        );
    });
}

pub(super) fn a_leafs_padding_insets_its_text_and_widens_it(_: &Instrumentation) {
    on_main(|| {
        let harness =
            Harness::launch(Application::new(Styled, Window::new("Styled", Size::new(360, 400))));
        let density = harness.density();
        let (plain, padded) = (harness.placed("badge"), harness.placed("padded"));
        let (plain, padded) = (plain.expect("badge placed"), padded.expect("padded placed"));
        assert_eq!(padded.width, plain.width + 16, "the box reserves the padding");
        // Left and right padding in pixels, against start and end in dp.
        let inset = |key: &str, (left, right): (i32, i32)| {
            let style = style_of(&harness, key);
            let px = |dp: i32| f64::from(crate::units::to_px(dp, density));
            (f64::from(style[10]) - px(left)).abs() < 0.5
                && (f64::from(style[11]) - px(right)).abs() < 0.5
        };
        assert!(inset("padded", (12, 4)), "the text is inset at its start");
        assert!(inset("badge", (0, 0)), "a plain label keeps its own");
        harness.with_registry(|registry| {
            registry.with_application(|application| application.set_locale(Locale::new("ar-EG")));
            registry.render(WindowId::PRIMARY).expect("rendered");
        });
        assert!(inset("padded", (4, 12)), "right to left, the start is the right");
    });
}

pub(super) fn night_mode_reaches_the_environment_and_keeps_the_views(
    instrumentation: &Instrumentation,
) {
    let before = on_main(|| {
        let harness =
            Harness::launch(Application::new(Styled, Window::new("Styled", Size::new(360, 400))));
        let identity = java(
            Class::Probe,
            "identity",
            "(Landroid/view/View;)I",
            &[Arg::Obj(&harness.expect("plain"))],
        )
        .int();
        let scheme = harness.with_application(|application| {
            application.environment_for(WindowId::PRIMARY, &keys::COLOR_SCHEME)
        });
        keep(harness);
        (identity, scheme)
    });
    // The person's own setting (`yes`, `no`, `auto`, `custom`), put back
    // exactly as it was.
    let original = instrumentation
        .shell("cmd uimode night")
        .trim()
        .rsplit(' ')
        .next()
        .unwrap_or("auto")
        .to_owned();
    let target = if before.1 == ColorScheme::Dark { "no" } else { "yes" };
    let _ = instrumentation.shell(&format!("cmd uimode night {target}"));
    let expected =
        if before.1 == ColorScheme::Dark { ColorScheme::Light } else { ColorScheme::Dark };
    instrumentation.wait_for("the scheme to change", Duration::from_secs(10), move || {
        with_kept(|harness| {
            harness.with_application(|application| {
                application.environment_for(WindowId::PRIMARY, &keys::COLOR_SCHEME)
            }) == expected
        })
    });
    let after = on_main(|| {
        with_kept(|harness| {
            java(
                Class::Probe,
                "identity",
                "(Landroid/view/View;)I",
                &[Arg::Obj(&harness.expect("plain"))],
            )
            .int()
        })
    });
    let _ = instrumentation.shell(&format!("cmd uimode night {original}"));
    assert_eq!(before.0, after, "the same view, restyled in place");
}

pub(super) fn font_scale_reaches_the_environment_and_remeasures(instrumentation: &Instrumentation) {
    let original = instrumentation.shell("settings get system font_scale").trim().to_owned();
    let height = on_main(|| {
        let harness =
            Harness::launch(Application::new(Styled, Window::new("Styled", Size::new(360, 400))));
        let height = harness.placed("caption").map_or(0, |rect| rect.height);
        keep(harness);
        height
    });
    let _ = instrumentation.shell("settings put system font_scale 1.5");
    let grown = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let seen = std::sync::Arc::clone(&grown);
    instrumentation.wait_for(
        "the font scale to reach the environment",
        Duration::from_secs(10),
        move || {
            with_kept(|harness| {
                let scale = harness.with_application(|application| {
                    application.environment_for(WindowId::PRIMARY, &keys::TEXT_SCALE)
                });
                seen.store(
                    harness.placed("caption").map_or(0, |rect| rect.height),
                    std::sync::atomic::Ordering::Relaxed,
                );
                scale == Scalar::new(1.5)
            })
        },
    );
    let restore =
        if original.is_empty() || original == "null" { "1.0".to_owned() } else { original };
    let _ = instrumentation.shell(&format!("settings put system font_scale {restore}"));
    let grown = grown.load(std::sync::atomic::Ordering::Relaxed);
    assert!(grown > height, "the caption is measured larger at 1.5× ({height} → {grown})");
}

pub(super) fn right_to_left_mirrors_with_the_same_views(_: &Instrumentation) {
    on_main(|| {
        let harness =
            Harness::launch(Application::new(Styled, Window::new("Styled", Size::new(360, 400))));
        let identity = java(
            Class::Probe,
            "identity",
            "(Landroid/view/View;)I",
            &[Arg::Obj(&harness.expect("styled"))],
        )
        .int();
        let ltr = (harness.frame("styled")[0], harness.frame("plain")[0]);
        assert!(ltr.0 < ltr.1, "left to right: the first button is on the left");
        harness.with_registry(|registry| {
            registry.with_application(|application| application.set_locale(Locale::new("ar-EG")));
            registry.render(WindowId::PRIMARY).expect("rendered");
        });
        assert_eq!(
            harness.with_application(|application| application.layout_direction(WindowId::PRIMARY)),
            LayoutDirection::Rtl
        );
        let rtl = (harness.frame("styled")[0], harness.frame("plain")[0]);
        assert!(rtl.0 > rtl.1, "right to left: the first button is on the right ({rtl:?})");
        assert!(
            (style_of(&harness, "styled")[7] - 1.0).abs() < f32::EPSILON,
            "the view draws right to left"
        );
        let after = java(
            Class::Probe,
            "identity",
            "(Landroid/view/View;)I",
            &[Arg::Obj(&harness.expect("styled"))],
        )
        .int();
        assert_eq!(identity, after, "the same view");
    });
}

pub(super) fn the_safe_area_is_the_windows_insets(instrumentation: &Instrumentation) {
    on_main(|| {
        keep(Harness::launch(Application::new(Styled, Window::new("Styled", Size::new(360, 400)))));
    });
    // The insets arrive from the window after it is laid out.
    instrumentation.wait_for("the window's insets", Duration::from_secs(10), || {
        with_kept(|harness| {
            harness.with_registry(|registry| registry.windows[&WindowId::PRIMARY].insets[1] > 0)
        })
    });
    on_main(|| {
        with_kept(|harness| {
            let (insets, density) = harness.with_registry(|registry| {
                let runtime = &registry.windows[&WindowId::PRIMARY];
                (runtime.insets, runtime.density)
            });
            let area = harness.with_application(|application| {
                application.environment_for(WindowId::PRIMARY, &keys::SAFE_AREA)
            });
            let top = crate::units::to_dp(insets[1].max(insets[5]), density);
            assert_eq!(u32::try_from(area.top).unwrap_or(0), top, "the status bar ({insets:?})");
            assert!(area.top > 0, "a phone has a status bar the content must clear");
            let bottom = crate::units::to_dp(insets[3].max(insets[7]).max(insets[11]), density);
            assert_eq!(u32::try_from(area.bottom).unwrap_or(0), bottom, "the navigation bar");
            // Content is placed clear of it, not just told about it.
            harness.settle();
            let root = harness.placed("root").expect("the root is placed");
            assert_eq!(root.y, area.top, "the content starts below the status bar");
        });
    });
}
