//! `PermissionService` on Linux.
//!
//! An application running unsandboxed (a distribution package, a tarball,
//! an AppImage) is not gated by the desktop at all: it has every resource
//! the person's account has, and every permission is `Granted`.
//!
//! Inside a sandbox (Flatpak, Snap) the portals gate two resources at use,
//! asking the person: the camera (the Camera portal's `AccessCamera`) and
//! the location (the Location portal's session). Their state is
//! `NotAsked` until a request in this run is answered — the portal's
//! permission store is not readable from inside the sandbox. Every other
//! resource is a static permission of the sandbox's manifest, granted at
//! install time.
//!
//! | Permission | Unsandboxed | Sandboxed |
//! |---|---|---|
//! | Camera | `Granted` | Camera portal, asked on request |
//! | Location | `Granted` | Location portal, asked on request |
//! | Microphone, Notifications, Contacts, Photos, Bluetooth | `Granted` | `Granted` (the manifest's) |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rustnative_core::{Permission, PermissionService, PermissionState, ServiceError};

use super::on_gtk;
use crate::desktop::portal::{self, Response};

/// Linux's permission service.
#[derive(Debug, Clone)]
pub struct LinuxPermissions {
    sandboxed: bool,
    /// The answers given in this run.
    answers: Arc<Mutex<HashMap<Permission, PermissionState>>>,
}

impl Default for LinuxPermissions {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxPermissions {
    /// The service, for this process's sandbox (or none).
    #[must_use]
    pub fn new() -> Self {
        Self::with_sandbox(portal::sandboxed())
    }

    fn with_sandbox(sandboxed: bool) -> Self {
        Self { sandboxed, answers: Arc::default() }
    }

    fn gated(&self, permission: Permission) -> bool {
        self.sandboxed && matches!(permission, Permission::Camera | Permission::Location)
    }
}

/// Asks the portal for `permission`.
async fn ask(permission: Permission) -> Result<Response, String> {
    use gio::prelude::*;
    let options = |token: &str| {
        let mut options: HashMap<String, glib::Variant> = HashMap::new();
        options.insert("handle_token".into(), token.to_variant());
        options
    };
    match permission {
        Permission::Camera => {
            portal::request("org.freedesktop.portal.Camera", "AccessCamera", |token| {
                (options(token),).to_variant()
            })
            .await
        }
        Permission::Location => {
            let connection = gio::bus_get_future(gio::BusType::Session)
                .await
                .map_err(|error| error.to_string())?;
            let session_token = format!("rustnative_location_{}", std::process::id());
            let session_options: HashMap<String, glib::Variant> =
                HashMap::from([("session_handle_token".to_owned(), session_token.to_variant())]);
            let reply = connection
                .call_future(
                    Some(portal::PORTAL),
                    portal::PATH,
                    "org.freedesktop.portal.Location",
                    "CreateSession",
                    Some(&(session_options,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    10_000,
                )
                .await
                .map_err(|error| format!("the Location portal refused a session: {error}"))?;
            let session: glib::variant::ObjectPath =
                reply.child_value(0).get().ok_or("the Location portal returned no session")?;
            let answer = portal::request("org.freedesktop.portal.Location", "Start", |token| {
                (session.clone(), "", options(token)).to_variant()
            })
            .await;
            // Asking was the point; the session is not kept running.
            let _ = connection
                .call_future(
                    Some(portal::PORTAL),
                    session.as_str(),
                    "org.freedesktop.portal.Session",
                    "Close",
                    None,
                    None,
                    gio::DBusCallFlags::NONE,
                    2_000,
                )
                .await;
            answer
        }
        _ => Ok(Response::Granted(HashMap::new())),
    }
}

#[async_trait::async_trait]
impl PermissionService for LinuxPermissions {
    fn state(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        if !self.gated(permission) {
            return Ok(PermissionState::Granted);
        }
        let answers = self.answers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(answers.get(&permission).copied().unwrap_or(PermissionState::NotAsked))
    }

    async fn request(&self, permission: Permission) -> Result<PermissionState, ServiceError> {
        if !self.gated(permission) {
            return Ok(PermissionState::Granted);
        }
        let response =
            on_gtk(move || async move { ask(permission).await.map_err(ServiceError::new) }).await?;
        let state = match response {
            Response::Granted(_) => PermissionState::Granted,
            Response::Denied => PermissionState::Denied,
            Response::Ended => PermissionState::NotAsked,
        };
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(permission, state);
        Ok(state)
    }

    fn open_settings(&self, permission: Permission) -> bool {
        // The desktop's own settings: GNOME Settings' panels, or Plasma's
        // modules; another desktop has no page to name.
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_ascii_uppercase();
        let (program, page) = if desktop.contains("GNOME") || desktop.contains("UNITY") {
            let panel = match permission {
                Permission::Camera => "camera",
                Permission::Microphone => "microphone",
                Permission::Location => "location",
                Permission::Notifications => "notifications",
                Permission::Bluetooth => "bluetooth",
                _ => "applications",
            };
            ("gnome-control-center", panel)
        } else if desktop.contains("KDE") {
            let module = match permission {
                Permission::Notifications => "kcm_notifications",
                Permission::Bluetooth => "kcm_bluetooth",
                _ => "kcm_app-permissions",
            };
            ("systemsettings", module)
        } else {
            return false;
        };
        std::process::Command::new(program).arg(page).spawn().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Permission; 7] = [
        Permission::Camera,
        Permission::Microphone,
        Permission::Location,
        Permission::Notifications,
        Permission::Contacts,
        Permission::Photos,
        Permission::Bluetooth,
    ];

    #[test]
    fn an_unsandboxed_application_is_not_gated() {
        let service = LinuxPermissions::with_sandbox(false);
        for permission in ALL {
            assert_eq!(
                service.state(permission).expect("answered"),
                PermissionState::Granted,
                "{permission:?}"
            );
        }
        let sandboxed = LinuxPermissions::with_sandbox(true);
        assert_eq!(
            sandboxed.state(Permission::Camera).expect("answered"),
            PermissionState::NotAsked
        );
        assert_eq!(
            sandboxed.state(Permission::Microphone).expect("answered"),
            PermissionState::Granted
        );
    }

    #[test]
    fn a_sandboxed_camera_request_asks_the_portal_and_remembers_the_answer() {
        use crate::desktop::fake_bus::{FakeService, Object};
        use gio::prelude::*;
        crate::gtk::testing::on_gtk(|| {
            let xml = r#"<node><interface name="org.freedesktop.portal.Camera">
                <method name="AccessCamera"><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method>
                </interface></node>"#;
            let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
                .expect("a session bus");
            let client = connection.clone();
            let portal = FakeService::start(
                portal::PORTAL,
                vec![Object {
                    path: portal::PATH.into(),
                    xml: xml.into(),
                    handler: Arc::new(move |_, parameters| {
                        let (options,) = parameters
                            .get::<(HashMap<String, glib::Variant>,)>()
                            .ok_or("options")?;
                        let token = options
                            .get("handle_token")
                            .and_then(glib::Variant::get::<String>)
                            .ok_or("a token")?;
                        let path = portal::request_path(&client, &token);
                        Ok((glib::variant::ObjectPath::try_from(path).map_err(|_| "a path")?,)
                            .to_variant())
                    }),
                }],
            );
            let service = LinuxPermissions::with_sandbox(true);
            let asking = service.clone();
            let (reply, answer) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let runtime =
                    tokio::runtime::Builder::new_current_thread().build().expect("a runtime");
                let _ = reply.send(runtime.block_on(asking.request(Permission::Camera)));
            });
            let mut call = None;
            crate::gtk::testing::pump_until(
                "the portal call",
                std::time::Duration::from_secs(5),
                || {
                    call = portal.calls_to("AccessCamera").pop();
                    call.is_some()
                },
            );
            let (options,) = call
                .expect("called")
                .parameters
                .get::<(HashMap<String, glib::Variant>,)>()
                .expect("options");
            let token = options
                .get("handle_token")
                .and_then(glib::Variant::get::<String>)
                .expect("a token");
            let results: HashMap<String, glib::Variant> = HashMap::new();
            portal.emit(
                &portal::request_path(&connection, &token),
                "org.freedesktop.portal.Request",
                "Response",
                (1_u32, results).to_variant(),
            );
            let mut state = None;
            crate::gtk::testing::pump_until(
                "the answer",
                std::time::Duration::from_secs(5),
                || {
                    state = answer.try_recv().ok();
                    state.is_some()
                },
            );
            assert_eq!(state.expect("answered").expect("no failure"), PermissionState::Denied);
            assert_eq!(
                service.state(Permission::Camera).expect("answered"),
                PermissionState::Denied,
                "remembered"
            );
        });
    }
}
