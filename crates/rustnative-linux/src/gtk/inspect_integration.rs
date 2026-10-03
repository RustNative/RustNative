//! Inspection on GTK (Phase 7): the realized widgets as the inspector sees
//! them, the lifetime census, and the overlay.

use gtk::prelude::*;
use rustnative_core::inspect::InspectBackend;
use rustnative_core::{Application, Component, Event, Node, Size, Window, WindowId};

use super::inspect::GtkInspect;
use super::testing::{Harness, on_gtk};

struct Form;

impl Component for Form {
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
        Node::column("root", [Node::label("title", "Title"), Node::button("save", "Save")])
    }
    fn update(&mut self, _event: Event) {}
}

#[test]
fn the_inspector_sees_each_node_s_widget_and_the_census_balances() {
    on_gtk(|| {
        let mut application = Application::new(Form, Window::new("Inspect", Size::new(320, 240)));
        // SAFETY: `application` outlives `harness`, declared after it.
        let harness = unsafe { Harness::attach(&mut application) };
        let (realized, lifetimes, rects, live) = harness.with_registry(|registry| {
            let runtime = &registry.windows[&WindowId::PRIMARY];
            let inspect = GtkInspect { runtime };
            (
                inspect.realized(WindowId::PRIMARY),
                inspect.lifetimes(),
                inspect.rects(WindowId::PRIMARY).map(|rects| rects.len()),
                runtime.renderer.registry.len(),
            )
        });
        let save = realized
            .iter()
            .find(|object| object.key.as_deref() == Some("save"))
            .expect("the button");
        assert_eq!(save.host_type, "GtkButton");
        assert!(save.handle.is_some() && save.rect.is_some_and(|rect| rect[2] > 0));
        assert_eq!(lifetimes.live, live as u64, "every live widget is counted once");
        assert_eq!(rects, Some(3));
        assert_eq!(harness.expect("save").type_().name(), "GtkButton");
    });
}
