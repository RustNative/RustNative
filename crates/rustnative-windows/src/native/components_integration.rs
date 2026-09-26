//! The component library on Windows (`PLAN.md` Milestone 48's "done
//! when"): an application built from `rustnative-components` and a token set
//! realizes as the system's own controls, and the roles its tokens mark
//! take the host's colors.

use rustnative_components::{
    ActionButton, ActionButtonProps, Badge, BadgeProps, ButtonVariant, Card, CardProps, Chart,
    ChartKind, ChartProps, Series, TextField, TextFieldProps, Tone, with_roles,
};
use rustnative_core::{
    Application, Component, ComponentContext, Event, Node, Size, StyleValue, Theme, Window,
    WindowId,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

use super::harness::NativeHarness;

struct Showcase;

impl Component for Showcase {
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
        Node::column("showcase", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let field = context.child_with_props::<TextField, _>(
            "name",
            TextFieldProps { label: "Name".into(), ..TextFieldProps::default() },
            TextField::new,
        );
        let save = context.child_with_props::<ActionButton, _>(
            "save",
            ActionButtonProps {
                text: "Save".into(),
                variant: ButtonVariant::Primary,
                command: None,
            },
            ActionButton::new,
        );
        let badge = context.child_with_props::<Badge, _>(
            "new",
            BadgeProps { text: "New".into(), tone: Tone::Accent },
            Badge::new,
        );
        let card = context.child_with_props::<Card, _>(
            "card",
            CardProps { title: "Status".into(), body: vec![badge] },
            Card::new,
        );
        let chart = context.child_with_props::<Chart, _>(
            "chart",
            ChartProps {
                kind: ChartKind::Line,
                title: "Trend".into(),
                categories: vec!["A".into(), "B".into()],
                series: vec![Series::new("s", [1.0, 2.0])],
                width: 200,
                height: 120,
            },
            Chart::new,
        );
        Node::column("showcase", [field, save, card, chart])
    }
}

fn class(hwnd: HWND) -> String {
    let mut buffer = [0_u16; 64];
    // SAFETY: a live window; the buffer's length is passed.
    let length = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), 64) };
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

#[test]
fn native_library_components_are_host_controls_in_host_colors() {
    let mut application =
        Application::new(Showcase::new(()), Window::new("components", Size::new(600, 700)));
    application.set_theme(with_roles(Theme::default()));
    // What `run` does before the first window (`host_traits::apply`); the
    // harness leaves host traits out so other tests do not depend on the
    // machine's settings.
    application.set_host_palette(super::host_traits::palette());
    // SAFETY: `application` is declared first, so it outlives the harness.
    let harness = unsafe { NativeHarness::attach(&mut application) };
    let control = |key| harness.expect_control(WindowId::PRIMARY, key);

    assert_eq!(class(control("input")), "Edit", "a text field is the system's edit");
    assert_eq!(class(control("button")), "Button", "an action is the system's button");
    assert_eq!(class(control("badge")), "Static");
    assert!(harness.control(WindowId::PRIMARY, "plot").is_some(), "the chart's canvas");

    let accent = harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime.with_application(|application| {
            application.theme().tokens().get("color-accent").cloned()
        })
    });
    assert_eq!(
        accent,
        Some(StyleValue::Color(super::host_traits::palette().accent)),
        "the accent role is the accent chosen in Settings"
    );
}
