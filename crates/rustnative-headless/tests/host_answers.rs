//! What the headless backend answers on a host's behalf: its capabilities,
//! the surfaces an application asks for, and text typed into any field.

use rustnative_core::surfaces::SurfaceCommand;
use rustnative_core::{
    Capability, Component, ComponentContext, Control, Event, LayoutStyle, Node,
    PlatformCapabilities, Size, SizeMode, Window,
};
use rustnative_headless::{HeadlessApp, Query};

/// Shows a copy button only where there is a clipboard, a notes field, and
/// posts a notification when the notes change.
#[derive(Default)]
struct Notes {
    notes: String,
    notify: bool,
}

impl Component for Notes {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self::default()
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("unused", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        if std::mem::take(&mut self.notify) {
            context.services().surfaces().notify("Saved", &self.notes);
        }
        let copy = if context.capabilities().supports(Capability::Clipboard) {
            Node::button("copy", "Copy")
        } else {
            Node::label("copy", "No clipboard here")
        };
        Node::column(
            "root",
            [
                copy,
                Node::control_with_layout(
                    "notes",
                    Control::MultilineText { value: self.notes.clone() },
                    LayoutStyle::new().width(SizeMode::Fixed(200)).height(SizeMode::Fixed(60)),
                ),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::TextChanged { value, .. } = event {
            self.notes = value;
            self.notify = true;
        }
    }
}

fn launch() -> HeadlessApp {
    HeadlessApp::launch(Window::new("Notes", Size::new(300, 200)), || Notes::new(()))
}

#[test]
fn components_read_the_capabilities_the_backend_answers() {
    let app = launch();
    assert_eq!(app.find(&Query::key("copy")).expect("copy").kind, rustnative_core::NodeKind::Label);
    let app = app.with_capabilities(PlatformCapabilities::new([Capability::Clipboard]));
    assert_eq!(
        app.find(&Query::key("copy")).expect("copy").kind,
        rustnative_core::NodeKind::Button
    );
}

#[test]
fn typing_reaches_a_multi_line_field_and_its_surfaces_are_recorded() {
    let mut app = launch();
    app.type_text(&Query::key("notes"), "hi").expect("a multi-line field takes typing");
    let notified = app.take_surface_commands();
    assert!(
        matches!(notified.last(), Some(SurfaceCommand::Notify { body, .. }) if body == "hi"),
        "{notified:?}"
    );
}
