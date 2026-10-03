//! The adoption ladder on Linux (`PLAN.md` Milestone 40): an existing GTK 4
//! application — its own window, widgets, and main loop — adopts Rust
//! Native without being rewritten:
//!
//! 1. it **embeds** a Rust Native subtree as a widget in its own layout
//!    (`LinuxPlatform::embed`), running under the host's loop;
//! 2. that subtree **adopts** one of the host's own widgets — its calendar —
//!    as a leaf of the declarative tree (`register_foreign`), borrowed, so
//!    the host gets it back when the subtree lets go.
//!
//! Run with `--self-test` to drive it and exit (what `tests/host.rs` does).

#[cfg(target_os = "linux")]
mod gtk_host {
    use std::time::{Duration, Instant};

    use gtk::prelude::*;
    use rustnative_core::{
        Application, Component, Event, LayoutStyle, Node, NodeId, Size, SizeMode, Window,
    };
    use rustnative_linux::{ForeignWidget, LinuxPlatform, Ownership, register_foreign};

    /// The Rust Native part: a planner panel that shows the host's
    /// calendar and counts the days planned.
    struct Planner {
        planned: u32,
    }

    impl Component for Planner {
        type Props = ();
        type Message = ();
        fn new((): ()) -> Self {
            Self { planned: 0 }
        }
        fn props(&self) -> &() {
            &()
        }
        fn set_props(&mut self, (): ()) {}
        fn view(&self) -> Node {
            Node::column(
                "planner",
                [
                    Node::label("planned", format!("Days planned: {}", self.planned)),
                    Node::button("plan", "Plan this day"),
                    // The host's own calendar, adopted as a leaf.
                    Node::foreign(
                        "calendar",
                        "host-calendar",
                        LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(220)),
                    ),
                ],
            )
        }
        fn update(&mut self, event: Event) {
            if matches!(event, Event::Click { target } if target == NodeId::from_key("plan")) {
                self.planned += 1;
            }
        }
    }

    /// The descendant of `widget` that is a `T` and satisfies `matches`.
    fn find<T: IsA<gtk::Widget>>(widget: &gtk::Widget, matches: &dyn Fn(&T) -> bool) -> Option<T> {
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

    fn iterate_until(what: &str, done: impl Fn() -> bool) -> Result<(), String> {
        let context = gtk::glib::MainContext::default();
        let until = Instant::now() + Duration::from_secs(20);
        while !done() {
            if Instant::now() > until {
                return Err(format!("timed out waiting for {what}"));
            }
            context.iteration(false);
        }
        Ok(())
    }

    pub fn main() {
        if let Err(error) = gtk::init() {
            eprintln!("no display: {error}");
            std::process::exit(1);
        }
        let self_test = std::env::args().any(|argument| argument == "--self-test");

        // The host as it was before Rust Native: a window, a heading, and a
        // calendar it keeps a reference to.
        let host = gtk::Window::builder()
            .title("Planner (a GTK host)")
            .default_width(420)
            .default_height(480)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&gtk::Label::new(Some("The host's own heading")));
        host.set_child(Some(&content));
        let calendar = gtk::Calendar::new();

        // Rung 2: the subtree may adopt the host's calendar, borrowed.
        let lent = calendar.clone();
        register_foreign("host-calendar", Size::new(300, 220), move || {
            Some(ForeignWidget { widget: lent.clone().upcast(), ownership: Ownership::Borrowed })
        });

        // Rung 1: the subtree, embedded in the host's layout.
        let mut application =
            Application::new(Planner::new(()), Window::new("Planner", Size::new(420, 400)));
        let root = match LinuxPlatform::new().embed(&mut application) {
            Ok(root) => root,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };
        root.widget().set_vexpand(true);
        content.append(root.widget());
        host.present();

        if !self_test {
            // The host's own loop; the subtree runs under it.
            let main_loop = gtk::glib::MainLoop::new(None, false);
            let quit = main_loop.clone();
            host.connect_close_request(move |_| {
                quit.quit();
                gtk::glib::Propagation::Proceed
            });
            main_loop.run();
            return;
        }

        let outcome = (|| -> Result<(), String> {
            iterate_until("the subtree to be laid out", || root.widget().height() > 0)?;
            if !calendar.is_ancestor(root.widget()) {
                return Err("the host's calendar is adopted into the subtree".to_owned());
            }
            let button = find::<gtk::Button>(root.widget(), &|button| {
                button.label().as_deref() == Some("Plan this day")
            })
            .ok_or("the subtree's button is realized")?;
            button.emit_clicked();
            iterate_until("the subtree to answer the click", || {
                find::<gtk::Label>(root.widget(), &|label| label.text() == "Days planned: 1")
                    .is_some()
            })?;
            Ok(())
        })();
        drop(root);
        drop(application);
        let outcome = outcome.and_then(|()| {
            // Borrowed: the host has its calendar back, out of any tree.
            if calendar.parent().is_some() {
                return Err("dropping the subtree hands the calendar back".to_owned());
            }
            Ok(())
        });
        host.destroy();
        match outcome {
            Ok(()) => println!("ok"),
            Err(problem) => {
                eprintln!("{problem}");
                std::process::exit(1);
            }
        }
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    gtk_host::main();
    #[cfg(not(target_os = "linux"))]
    eprintln!("adoption-gtk is a GTK host; on Windows, run adoption-subtree and adoption-foreign");
}
