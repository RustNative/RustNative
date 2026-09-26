//! The command model: one declaration, bound by buttons, menus, and
//! shortcuts, routed through the focus chain.

use rustnative_core::{
    Application, Command, CommandId, Component, ComponentContext, Event, KeyCode, KeyModifiers,
    MenuBar, MenuItem, Node, NodeId, Shortcut, Size, Window, WindowId,
};

const SAVE: CommandId = CommandId::new("test.save");

struct Editor {
    dirty: bool,
    saves: u32,
}

impl Component for Editor {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { dirty: false, saves: 0 }
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
            Event::Command { id } if id == SAVE => {
                self.saves += 1;
                self.dirty = false;
            }
            Event::TextChanged { .. } => self.dirty = true,
            _ => {}
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.command(
            Command::new(SAVE, "Save")
                .shortcut(Shortcut::ctrl(KeyCode::Character('s')))
                .enabled(self.dirty),
        );
        Node::column(
            "root",
            [
                Node::text_input("body", ""),
                Node::button("save", "Save").with_command(SAVE),
                Node::label("saves", format!("saves: {}", self.saves)),
            ],
        )
    }
}

fn app() -> Application {
    Application::new(
        Editor::new(()),
        Window::new("Editor", Size::new(300, 200)).with_menu(MenuBar::new([MenuItem::submenu(
            "file",
            "File",
            [MenuItem::command("file-save", "Save", SAVE)],
        )])),
    )
}

fn button_disabled(application: &Application) -> bool {
    let mut disabled = false;
    application.view().visit(&mut |node, _, _| {
        if node.command() == Some(SAVE) {
            disabled = node.is_disabled();
        }
    });
    disabled
}

fn saves(application: &Application) -> String {
    let mut text = String::new();
    application.view().visit(&mut |node, _, _| {
        if let Node::Label(label) = node {
            label.text().clone_into(&mut text);
        }
    });
    text
}

#[test]
fn a_disabled_command_disables_every_node_bound_to_it() {
    let mut application = app();
    assert!(button_disabled(&application), "nothing to save yet");
    application
        .dispatch(Event::TextChanged { target: NodeId::from_key("body"), value: "x".into() });
    assert!(!button_disabled(&application));
    let state = application.command_state(WindowId::PRIMARY, SAVE, None).expect("declared");
    assert!(state.is_enabled());
    assert_eq!(state.shortcut_label().as_deref(), Some("Ctrl+S"));
}

#[test]
fn a_button_a_menu_item_and_a_shortcut_invoke_the_same_command() {
    let mut application = app();
    application
        .dispatch(Event::TextChanged { target: NodeId::from_key("body"), value: "x".into() });
    application.dispatch(Event::Click { target: NodeId::from_key("save") });
    assert_eq!(saves(&application), "saves: 1");

    application
        .dispatch(Event::TextChanged { target: NodeId::from_key("body"), value: "y".into() });
    application.dispatch(Event::MenuAction {
        window: WindowId::PRIMARY,
        item: NodeId::from_key("file-save"),
    });
    assert_eq!(saves(&application), "saves: 2");

    application
        .dispatch(Event::TextChanged { target: NodeId::from_key("body"), value: "z".into() });
    let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
    assert!(application.handle_shortcut(WindowId::PRIMARY, KeyCode::Character('S'), ctrl, None));
    assert_eq!(saves(&application), "saves: 3");

    // Now clean: the command is disabled, so its shortcut is not taken and
    // the key press goes on to be an ordinary key event.
    assert!(!application.handle_shortcut(WindowId::PRIMARY, KeyCode::Character('s'), ctrl, None));
}
