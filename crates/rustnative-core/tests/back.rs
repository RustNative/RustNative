//! System back on the navigation model: a stack that can pop declares the
//! back command, a host asks before claiming its gesture, and predictive
//! progress reaches the declaring component before the command does.

use rustnative_core::command::standard::BACK;
use rustnative_core::{
    Application, BackEdge, BackPhase, Command, Component, ComponentContext, Event, Node, Scalar,
    Size, Window, WindowId,
};

struct Screens {
    depth: u32,
    progress: Vec<BackPhase>,
}

impl Component for Screens {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { depth: 1, progress: Vec::new() }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("root", [])
    }
    fn update(&mut self, event: Event) {
        match event {
            Event::Command { id } if id == BACK => self.depth -= 1,
            Event::BackProgress { phase, .. } => self.progress.push(phase),
            Event::Click { .. } => self.depth += 1,
            _ => {}
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        // Back is enabled while there is a screen to go back to.
        context.command(Command::new(BACK, "Back").enabled(self.depth > 1));
        Node::column(
            "root",
            [
                Node::label("depth", format!("depth {}", self.depth)),
                Node::label("progress", format!("{}", self.progress.len())),
                Node::button("push", "Push"),
            ],
        )
    }
}

fn label(application: &Application, key: &str) -> String {
    let mut text = String::new();
    let id = rustnative_core::NodeId::from_key(key);
    application.view().visit(&mut |node, _, _| {
        if let Node::Label(label) = node {
            if label.id() == id {
                label.text().clone_into(&mut text);
            }
        }
    });
    text
}

#[test]
fn back_is_the_hosts_until_there_is_somewhere_to_go_back_to() {
    let mut application =
        Application::new(Screens::new(()), Window::new("Screens", Size::new(300, 200)));
    // At the root, the host keeps its own back (leaving the application).
    assert!(!application.handles_back(WindowId::PRIMARY, None));
    assert!(!application.back_progress(
        WindowId::PRIMARY,
        BackPhase::Started { edge: BackEdge::Left },
        None
    ));
    assert!(!application.invoke_command(WindowId::PRIMARY, BACK, None));

    application.dispatch(Event::Click { target: rustnative_core::NodeId::from_key("push") });
    assert_eq!(label(&application, "depth"), "depth 2");
    assert!(application.handles_back(WindowId::PRIMARY, None));

    // A predictive gesture reports its progress, then completes as the
    // command.
    for phase in [
        BackPhase::Started { edge: BackEdge::Left },
        BackPhase::Progressed { progress: Scalar::new(0.5) },
    ] {
        assert!(application.back_progress(WindowId::PRIMARY, phase, None));
    }
    assert_eq!(label(&application, "progress"), "2");
    assert!(application.invoke_command(WindowId::PRIMARY, BACK, None));
    assert_eq!(label(&application, "depth"), "depth 1");
    assert!(!application.handles_back(WindowId::PRIMARY, None));
}

#[test]
fn taking_an_application_leaves_an_empty_one_and_moves_every_window() {
    let mut application =
        Application::new(Screens::new(()), Window::new("Screens", Size::new(300, 200)));
    application.dispatch(Event::Click { target: rustnative_core::NodeId::from_key("push") });
    let mut adopted = application.take();
    assert!(application.window_ids().is_empty(), "the taken-from application keeps no windows");
    assert!(
        !application.dispatch(Event::Click { target: rustnative_core::NodeId::from_key("push") })
    );
    // The taken application is the same one, state and all.
    assert_eq!(label(&adopted, "depth"), "depth 2");
    adopted.dispatch(Event::Click { target: rustnative_core::NodeId::from_key("push") });
    assert_eq!(label(&adopted, "depth"), "depth 3");
}
