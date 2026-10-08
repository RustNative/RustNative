//! Milestone 57's example: an application that lives partly outside its
//! window, with no native code of its own.
//!
//! - A **tray icon** with a menu; choosing "Export" there does what the
//!   button does.
//! - A **jump list** task that starts a new note.
//! - **Taskbar progress** while an export runs, and a **notification**
//!   when it ends; clicking the notification comes back as an action.
//! - A sign-in **token kept in secure storage**, read back at start.
//! - A **remotely toggled feature**: the compact layout, off until the
//!   remote configuration turns it on.

use std::sync::Arc;

use rustnative_core::capability::SurfaceKind;
use rustnative_core::product::{Flag, SecureStorage};
use rustnative_core::surfaces::{JumpTask, NOTIFICATION, Surfaces, TrayMenuItem};
use rustnative_core::{Component, ComponentContext, Event, Node, NodeId};

/// The remotely toggled feature.
pub const COMPACT: Flag<bool> = Flag::new("compact-layout", false);

/// The secure-storage entry holding the sign-in token.
pub const TOKEN: &str = "sign-in-token";

/// The application.
pub struct Product {
    surfaces: Option<Surfaces>,
    secrets: Option<Arc<dyn SecureStorage>>,
    compact: bool,
    progress: u8,
    signed_in: bool,
    last_action: String,
}

impl Product {
    fn export_step(&mut self) {
        let Some(surfaces) = &self.surfaces else { return };
        self.progress = (self.progress + 25).min(100);
        if self.progress < 100 {
            surfaces.set_progress(Some(f32::from(self.progress) / 100.0));
        } else {
            surfaces.set_progress(None);
            surfaces.notify("Export finished", "Your notes were exported.");
            self.progress = 0;
        }
    }
}

impl Component for Product {
    type Props = ();
    type Message = ();

    fn new((): ()) -> Self {
        Self {
            surfaces: None,
            secrets: None,
            compact: false,
            progress: 0,
            signed_in: false,
            last_action: String::new(),
        }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column("product", [])
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::Click { target } if target == NodeId::from_key("export") => self.export_step(),
            Event::Click { target } if target == NodeId::from_key("sign-in") => {
                if let Some(secrets) = &self.secrets {
                    self.signed_in = secrets.put(TOKEN, b"token-from-the-server").is_ok();
                }
            }
            Event::Click { target } if target == NodeId::from_key("sign-out") => {
                if let Some(secrets) = &self.secrets {
                    self.signed_in = secrets.delete(TOKEN).is_err();
                }
            }
            Event::SurfaceAction { surface: SurfaceKind::TrayExtra, action, .. } => {
                if action == "export" {
                    self.export_step();
                }
                self.last_action = action;
            }
            _ => {}
        }
    }

    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let services = context.services();
        if self.surfaces.is_none() {
            // Once: the tray icon and the jump list, and whether a token
            // survived from the last run.
            let surfaces = services.surfaces().clone();
            surfaces.show_tray(
                "Product services",
                vec![TrayMenuItem::new("export", "Export"), TrayMenuItem::new("open", "Open")],
            );
            surfaces.set_jump_list(vec![JumpTask {
                label: "New note".into(),
                arguments: "product://new".into(),
            }]);
            self.surfaces = Some(surfaces);
            self.secrets = services.secure_storage().cloned();
            self.signed_in = self
                .secrets
                .as_ref()
                .is_some_and(|secrets| matches!(secrets.get(TOKEN), Ok(Some(_))));
        }
        self.compact = services.flags().get(&COMPACT);

        let status = match self.last_action.as_str() {
            NOTIFICATION => "You came back from the notification.".to_owned(),
            "" => String::new(),
            other => format!("Tray: {other}"),
        };
        let account = if self.signed_in {
            Node::button("sign-out", "Sign out")
        } else {
            Node::button("sign-in", "Sign in")
        };
        let children = [
            Node::label("title", if self.compact { "Notes (compact)" } else { "Notes" }),
            Node::button("export", format!("Export ({}%)", self.progress)),
            account,
            Node::label("status", status),
        ];
        if self.compact {
            Node::row("product", children)
        } else {
            Node::column("product", children)
        }
    }
}

/// The Android entry: Keystore-backed secure storage, and push and billing
/// answering why they are unavailable here.
#[cfg(target_os = "android")]
mod android {
    use std::sync::Arc;

    use rustnative_core::product::Flags;
    use rustnative_core::{Application, Component, Platform, Services, Size, Window};

    fn main() -> Result<(), rustnative_android::Error> {
        let services = Services::default()
            .with_flags(Flags::new())
            .with_secure_storage(Arc::new(rustnative_android::AndroidSecureStorage))
            .with_push(Arc::new(rustnative_android::AndroidPush))
            .with_commerce(Arc::new(rustnative_android::AndroidStore));
        let mut application = Application::with_services(
            super::Product::new(()),
            Window::new("Product services", Size::new(480, 320)),
            services,
        );
        rustnative_android::AndroidPlatform::new().run(&mut application)
    }

    rustnative_android::export_main!(main);
}
