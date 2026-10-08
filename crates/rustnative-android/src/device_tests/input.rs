//! Keys, system back, game controllers, and input methods on a real device
//! (Phase 3): input is injected through the instrumentation, so it arrives
//! the way a person's does.

use std::time::Duration;

use rustnative_core::command::standard::BACK;
use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, Application, Command, Component, ComponentContext,
    Composition, Event, GamepadButton, GamepadInput, InputInterest, Node, Size, Window,
};

use super::harness::{Harness, Instrumentation, java, keep, on_main, with_kept};
use crate::jni_host::{Arg, Class, call};

/// Records what input reached it, one line per event.
struct Recorder {
    seen: Vec<String>,
    depth: u32,
}

impl Component for Recorder {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { seen: Vec::new(), depth: 2 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::column("pad", []).with_input(InputInterest::new().gamepad()),
                Node::column("editor", []).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::TextInput)
                        .name("Editor")
                        .focusable(true),
                ),
                Node::label("seen", self.seen.join("\n")),
                Node::label("depth", format!("{}", self.depth)),
            ],
        )
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.command(Command::new(BACK, "Back").enabled(self.depth > 1));
        self.view()
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::KeyDown { key, .. } => self.seen.push(format!("down {key:?}")),
            Event::Command { id } if id == BACK => self.depth -= 1,
            Event::Gamepad {
                input: GamepadInput::Button { button: GamepadButton::South, pressed },
                ..
            } => {
                self.seen.push(format!("south {pressed}"));
            }
            Event::Composition { composition, .. } => self.seen.push(match composition {
                Composition::Started => "compose".to_owned(),
                Composition::Updated { text, .. } => format!("composing {text}"),
                Composition::Committed { text } => format!("committed {text}"),
                Composition::Cancelled => "cancelled".to_owned(),
            }),
            Event::TextInput { text, .. } => self.seen.push(format!("text {text}")),
            _ => {}
        }
    }
}

fn launch() {
    on_main(|| {
        keep(Harness::launch(Application::new(
            Recorder::new(()),
            Window::new("Input", Size::new(360, 400)),
        )));
    });
}

fn seen() -> String {
    on_main(|| with_kept(|harness| harness.text("seen").unwrap_or_default()))
}

pub(super) fn a_key_reaches_the_application(instrumentation: &Instrumentation) {
    launch();
    // KEYCODE_A.
    let _ = call(&instrumentation.0, "sendKeyDownUpSync", "(I)V", &[Arg::Int(29)]);
    instrumentation.wait_for("the key", Duration::from_secs(5), || {
        on_main(|| {
            with_kept(|harness| harness.text("seen").is_some_and(|seen| seen.contains("down")))
        })
    });
}

pub(super) fn system_back_is_the_back_command_while_it_is_enabled(
    instrumentation: &Instrumentation,
) {
    launch();
    let depth = || on_main(|| with_kept(|harness| harness.text("depth").unwrap_or_default()));
    let _ = call(&instrumentation.0, "sendKeyDownUpSync", "(I)V", &[Arg::Int(4)]);
    instrumentation.wait_for("back", Duration::from_secs(5), move || {
        on_main(|| with_kept(|harness| harness.text("depth").as_deref() == Some("1")))
    });
    assert_eq!(depth(), "1", "back went back one screen");
    // With nothing to go back to, BACK is disabled and the activity's own
    // back would happen — not pressed here, since it would end the suite's
    // activity.
    let handled = on_main(|| {
        with_kept(|harness| {
            harness.with_application(|application| {
                application.handles_back(rustnative_core::WindowId::PRIMARY, None)
            })
        })
    });
    assert!(!handled, "back is the system's once the stack is at its root");
}

pub(super) fn a_controller_button_reaches_the_node_that_wants_it(
    instrumentation: &Instrumentation,
) {
    launch();
    // KEYCODE_BUTTON_A.
    let _ = call(&instrumentation.0, "gamepadButton", "(I)V", &[Arg::Int(96)]);
    instrumentation.wait_for("the button", Duration::from_secs(5), || {
        on_main(|| {
            with_kept(|harness| {
                harness.text("seen").is_some_and(|seen| seen.contains("south false"))
            })
        })
    });
    let seen = seen();
    assert!(seen.contains("south true"), "the press, then the release: {seen}");
}

pub(super) fn an_input_method_composes_into_a_custom_text_target(
    instrumentation: &Instrumentation,
) {
    launch();
    on_main(|| {
        with_kept(|harness| {
            let editor = harness.expect("editor");
            let _ = call(&editor, "requestFocus", "()Z", &[]);
            harness.pump();
            let connected = java(
                Class::Instrumentation,
                "ime",
                "(Landroid/view/View;Ljava/lang/String;Ljava/lang/String;)Z",
                &[Arg::Obj(&editor), Arg::Str("he"), Arg::Str("hello")],
            )
            .bool();
            assert!(connected, "a focused text target offers an input method a connection");
        });
    });
    instrumentation.wait_for("the commit", Duration::from_secs(5), || {
        on_main(|| {
            with_kept(|harness| harness.text("seen").is_some_and(|seen| seen.contains("committed")))
        })
    });
    let seen = seen();
    assert!(
        seen.contains("compose\ncomposing he\ncommitted hello"),
        "the composition's steps, in order: {seen}"
    );
}
