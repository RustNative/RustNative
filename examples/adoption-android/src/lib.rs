//! Milestone 40 on Android: the Rust Native half of an existing Android
//! application. The host's own activity (`java/…/HostActivity.java`) puts a
//! `RustNativeView` in its layout; when it attaches, this library's `main`
//! runs and `AndroidPlatform::run` realizes the application into that view.
//!
//! Its count is persisted state, so it also shows the lifecycle contract:
//! kill the process in the background (`adb shell am kill …`) and the count
//! is back when the host activity returns.

use rustnative_core::{Component, ComponentContext, Event, Node, Persisted};

/// A counter: proof the embedded tree is live, takes input, and keeps its
/// state across process death.
pub struct Counter {
    count: Option<Persisted<u32>>,
}

impl Component for Counter {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { count: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::label("count", "")
    }
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let count = context.persisted("count", 0u32);
        let shown = count.get();
        self.count = Some(count);
        Node::column(
            "root",
            [
                Node::label("title", "Rendered by Rust Native, inside the host's activity"),
                Node::label("count", format!("Pressed {shown} times")),
                Node::button("press", "Press"),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            if let Some(count) = &self.count {
                count.update(|count| *count += 1);
            }
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::Arc;

    use rustnative_android::FileStateStore;
    use rustnative_core::{Application, Component, Platform as _, Services, Size, Window};

    fn main() -> Result<(), Box<dyn std::error::Error>> {
        let services =
            Services::default().with_state_store(Arc::new(FileStateStore::for_application()?));
        let mut application = Application::with_services(
            super::Counter::new(()),
            Window::new("Embedded", Size::new(360, 400)),
            services,
        );
        rustnative_android::AndroidPlatform::new().run(&mut application)?;
        Ok(())
    }

    rustnative_android::export_main!(main);
}
