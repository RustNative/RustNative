//! Embedding inward on GTK (Milestone 40, Phase 7): a GTK host places the
//! application's primary window as a widget in its own tree, the embedded
//! tree works under the host's loop, and dropping the root leaves the host
//! as it was.

use std::time::Duration;

use gtk::prelude::*;
use rustnative_core::{Application, Component, Event, Node, NodeId, Size, Window};

use super::testing::{on_gtk, pump_for, pump_until};

struct Counter {
    count: u32,
}

impl Component for Counter {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { count: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [Node::button("more", "More"), Node::label("count", format!("count: {}", self.count))],
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { target } if target == NodeId::from_key("more")) {
            self.count += 1;
        }
    }
}

/// The descendant of `widget` that is a `T` and satisfies `matches`.
fn find<T: IsA<gtk::Widget>>(widget: &gtk::Widget, matches: &impl Fn(&T) -> bool) -> Option<T> {
    if let Some(found) = widget.downcast_ref::<T>().filter(|candidate| matches(candidate)) {
        return Some(found.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find(&current, matches) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

#[test]
fn a_gtk_host_places_the_application_as_a_widget_and_gets_its_tree_back() {
    on_gtk(|| {
        let host = gtk::Window::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&gtk::Label::new(Some("The host's own label")));
        host.set_child(Some(&content));
        host.set_default_size(320, 240);

        let mut application =
            Application::new(Counter::new(()), Window::new("Embedded", Size::new(320, 200)));
        let root = crate::LinuxPlatform::new().embed(&mut application).expect("embedded");
        root.widget().set_vexpand(true);
        content.append(root.widget());
        host.present();
        pump_until("the embedded tree to be laid out", Duration::from_secs(20), || {
            root.widget().width() > 0 && root.widget().height() > 0
        });

        let button =
            find::<gtk::Button>(root.widget(), &|button| button.label().as_deref() == Some("More"))
                .expect("the button");
        button.emit_clicked();
        pump_for(Duration::from_millis(50));
        let label = find::<gtk::Label>(root.widget(), &|label| label.text().starts_with("count:"))
            .expect("the label");
        assert_eq!(label.text(), "count: 1", "the embedded tree works under the host's loop");
        assert!(!root.finished());

        drop(root);
        pump_for(Duration::from_millis(30));
        let children: Vec<gtk::Widget> =
            std::iter::successors(content.first_child(), gtk::Widget::next_sibling).collect();
        assert_eq!(children.len(), 1, "only the host's label is left");
        assert!(children[0].is::<gtk::Label>());
        drop(application);
        host.destroy();
    });
}
