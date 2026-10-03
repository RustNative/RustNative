//! Menu bars on GTK (Phase 6): the bar is in the window above the content,
//! a selection reaches the root component, and command-bound items follow
//! their command.

use gtk::prelude::*;
use rustnative_core::{
    Application, Command, CommandId, Component, ComponentContext, Event, KeyCode, KeyModifiers,
    MenuBar, MenuItem, Node, NodeId, Shortcut, Size, Window, WindowId,
};

use super::testing::{Harness, on_gtk};

const REFRESH: CommandId = CommandId::new("test.refresh");

/// Declares one command, bound to F5 and to a menu item, enabled only
/// after a first click; counts plain menu selections too.
struct Refresher {
    armed: bool,
    refreshes: u32,
    opened: u32,
}

impl Component for Refresher {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { armed: false, refreshes: 0, opened: 0 }
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
            Event::Command { id } if id == REFRESH => self.refreshes += 1,
            Event::MenuAction { item, .. } if item == NodeId::from_key("file-open") => {
                self.opened += 1;
            }
            Event::Click { .. } => self.armed = true,
            _ => {}
        }
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        context.command(
            Command::new(REFRESH, "Refresh")
                .shortcut(Shortcut::new(KeyCode::Function(5), KeyModifiers::default()))
                .enabled(self.armed),
        );
        Node::column(
            "root",
            [
                Node::button("arm", "Arm"),
                Node::label(
                    "count",
                    format!("refreshes: {} opened: {}", self.refreshes, self.opened),
                ),
            ],
        )
    }
}

fn menu() -> MenuBar {
    MenuBar::new([
        MenuItem::submenu(
            "file",
            "File",
            [
                MenuItem::action("file-open", "Open"),
                MenuItem::separator(),
                MenuItem::action("file-quit", "Quit").enabled(false),
            ],
        ),
        MenuItem::submenu("view", "View", [MenuItem::command("view-refresh", "Refresh", REFRESH)]),
    ])
}

#[test]
fn the_menu_bar_sits_above_the_content_and_its_items_reach_the_component() {
    on_gtk(|| {
        let mut application = Application::new(
            Refresher::new(()),
            Window::new("Menus", Size::new(320, 240)).with_menu(menu()),
        );
        // SAFETY: `application` outlives `harness`, declared after it.
        let harness = unsafe { Harness::attach(&mut application) };
        let window = harness.gtk_window(WindowId::PRIMARY).expect("a window");
        let content =
            window.child().and_downcast::<gtk::Box>().expect("the bar and the content in a box");
        let bar =
            content.first_child().and_downcast::<gtk::PopoverMenuBar>().expect("the bar first");
        let model = bar.menu_model().expect("a model");
        assert_eq!(model.n_items(), 2);
        assert_eq!(
            model
                .item_attribute_value(0, "label", Some(gtk::glib::VariantTy::STRING))
                .and_then(|v| v.get::<String>())
                .as_deref(),
            Some("File")
        );
        // The content is laid out below the bar.
        let root = harness.expect("root");
        assert!(
            root.compute_bounds(&window)
                .is_some_and(|bounds| f64::from(bounds.y()) >= f64::from(bar.height()))
        );

        let (open, quit) = harness.with_registry(|registry| {
            let menu = registry.window_menu(WindowId::PRIMARY).expect("realized");
            (
                menu.action_of(NodeId::from_key("file-open")).expect("open"),
                menu.action_of(NodeId::from_key("file-quit")).expect("quit"),
            )
        });
        assert!(open.is_enabled() && !quit.is_enabled(), "declared enabled state");
        // Activated as the bar activates it: the window's action.
        WidgetExt::activate_action(&window, &format!("rn.{}", open.name()), None)
            .expect("an action");
        harness.pump();
        assert_eq!(harness.expect_as::<gtk::Label>("count").text(), "refreshes: 0 opened: 1");
    });
}

#[test]
fn a_command_bound_item_follows_its_command() {
    on_gtk(|| {
        let mut application = Application::new(
            Refresher::new(()),
            Window::new("Menus", Size::new(320, 240)).with_menu(menu()),
        );
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        let refresh = NodeId::from_key("view-refresh");
        let (action, accel) = harness.with_registry(|registry| {
            let menu = registry.window_menu(WindowId::PRIMARY).expect("realized");
            (menu.action_of(refresh).expect("bound"), menu.accel_of(refresh))
        });
        assert!(!action.is_enabled(), "disabled until armed");
        assert_eq!(accel.as_deref(), Some("F5"), "the shortcut is shown");

        harness.click("arm");
        assert!(action.is_enabled(), "enabled once armed");
        let window = harness.gtk_window(WindowId::PRIMARY).expect("a window");
        WidgetExt::activate_action(&window, &format!("rn.{}", action.name()), None)
            .expect("an action");
        harness.pump();
        assert_eq!(harness.expect_as::<gtk::Label>("count").text(), "refreshes: 1 opened: 0");
    });
}
