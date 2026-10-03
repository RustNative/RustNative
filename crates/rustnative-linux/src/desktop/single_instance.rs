//! One running instance per application, and deep links handed from a
//! second launch to the first, over the session bus.
//!
//! The first instance owns the application id as a well-known name and
//! serves the freedesktop `org.freedesktop.Application` interface at the
//! id's object path — the interface a `DBusActivatable` desktop entry and
//! GApplication use, so the desktop can hand the running instance a URL
//! directly. A later launch finds the name owned, calls `Open` (with its
//! URL) or `Activate` (without one) on it, passing the launch's activation
//! token so the compositor lets the first instance's window come forward,
//! and exits.
//!
//! The session bus is per login session, so two people signed in to one
//! machine each get their own instance.

use std::collections::HashMap;

use gio::prelude::*;

const INTERFACE: &str = "org.freedesktop.Application";
const XML: &str = r#"<node><interface name="org.freedesktop.Application">
  <method name="Activate"><arg type="a{sv}" name="platform_data" direction="in"/></method>
  <method name="Open"><arg type="as" name="uris" direction="in"/><arg type="a{sv}" name="platform_data" direction="in"/></method>
  <method name="ActivateAction"><arg type="s" name="action_name" direction="in"/><arg type="av" name="parameter" direction="in"/><arg type="a{sv}" name="platform_data" direction="in"/></method>
</interface></node>"#;

/// What a second launch asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activation {
    /// The URL it was launched with, if any.
    pub url: Option<String>,
    /// The launch's activation token (`XDG_ACTIVATION_TOKEN` on Wayland,
    /// `DESKTOP_STARTUP_ID` on X11), for bringing the window forward.
    pub token: Option<String>,
}

/// The claim on being the one instance.
#[derive(Debug)]
pub enum Claim {
    /// This is the first instance; dropping it gives the name up.
    First(Instance),
    /// Another instance runs.
    AlreadyRunning,
    /// There is no session bus (or the id is not a bus name): every launch
    /// runs on its own.
    Unavailable,
}

/// The first instance's registration.
#[derive(Debug)]
pub struct Instance {
    connection: gio::DBusConnection,
    registration: Option<gio::RegistrationId>,
    name: String,
}

impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(registration) = self.registration.take() {
            let _ = self.connection.unregister_object(registration);
        }
        let _ = self.connection.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ReleaseName",
            Some(&(self.name.as_str(),).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            2_000,
            gio::Cancellable::NONE,
        );
    }
}

/// The object path for `app_id` (`com.example.Notes` → `/com/example/Notes`).
#[must_use]
pub fn object_path(app_id: &str) -> String {
    format!("/{}", app_id.replace('.', "/").replace('-', "_"))
}

/// Whether `app_id` is usable as a well-known bus name.
#[must_use]
pub fn is_bus_name(app_id: &str) -> bool {
    gio::dbus_is_name(app_id) && !gio::dbus_is_unique_name(app_id)
}

fn platform_data(token: Option<&str>) -> HashMap<String, glib::Variant> {
    let mut data = HashMap::new();
    if let Some(token) = token {
        data.insert("activation-token".to_owned(), token.to_variant());
        data.insert("desktop-startup-id".to_owned(), token.to_variant());
    }
    data
}

fn token_of(data: &HashMap<String, glib::Variant>) -> Option<String> {
    ["activation-token", "desktop-startup-id"]
        .iter()
        .find_map(|key| data.get(*key).and_then(glib::Variant::get::<String>))
}

/// Claims the single instance of `app_id`; `activated` hears every later
/// launch. Runs on the thread whose main context delivers `activated`.
pub fn claim(app_id: &str, activated: impl Fn(Activation) + 'static) -> Claim {
    match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(connection) => claim_on(&connection, app_id, activated),
        Err(_) => Claim::Unavailable,
    }
}

fn claim_on(
    connection: &gio::DBusConnection,
    app_id: &str,
    activated: impl Fn(Activation) + 'static,
) -> Claim {
    if !is_bus_name(app_id) {
        return Claim::Unavailable;
    }
    // The object is served before the name is owned, so a launch that
    // sees the name always finds the interface.
    let Ok(info) = gio::DBusNodeInfo::for_xml(XML) else { return Claim::Unavailable };
    let Some(interface) = info.lookup_interface(INTERFACE) else { return Claim::Unavailable };
    let registration = connection
        .register_object(&object_path(app_id), &interface)
        .method_call(move |_, _, _, _, method, parameters, invocation| {
            let activation = match method {
                "Open" => parameters.get::<(Vec<String>, HashMap<String, glib::Variant>)>().map(
                    |(uris, data)| Activation {
                        url: uris.into_iter().next(),
                        token: token_of(&data),
                    },
                ),
                "Activate" => parameters
                    .get::<(HashMap<String, glib::Variant>,)>()
                    .map(|(data,)| Activation { url: None, token: token_of(&data) }),
                "ActivateAction" => parameters
                    .get::<(String, Vec<glib::Variant>, HashMap<String, glib::Variant>)>()
                    .map(|(_, _, data)| Activation { url: None, token: token_of(&data) }),
                _ => None,
            };
            if let Some(activation) = activation {
                activated(activation);
            }
            invocation.return_value(None);
        })
        .build()
        .ok();
    let reply = connection.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        // DBUS_NAME_FLAG_DO_NOT_QUEUE
        Some(&(app_id, 4_u32).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        2_000,
        gio::Cancellable::NONE,
    );
    match reply.ok().and_then(|reply| reply.child_value(0).get::<u32>()) {
        // DBUS_REQUEST_NAME_REPLY_PRIMARY_OWNER, or already ours.
        Some(1 | 4) => Claim::First(Instance {
            connection: connection.clone(),
            registration,
            name: app_id.to_owned(),
        }),
        Some(_) => {
            if let Some(registration) = registration {
                let _ = connection.unregister_object(registration);
            }
            Claim::AlreadyRunning
        }
        None => Claim::Unavailable,
    }
}

/// This launch's activation token, from the environment the launcher set.
#[must_use]
pub fn launch_token() -> Option<String> {
    ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

/// Hands `url` (or just "come forward") to the running instance of
/// `app_id`. Returns whether it answered.
#[must_use]
pub fn forward(app_id: &str, url: Option<&str>, token: Option<&str>) -> bool {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .is_ok_and(|connection| forward_on(&connection, app_id, url, token))
}

fn forward_on(
    connection: &gio::DBusConnection,
    app_id: &str,
    url: Option<&str>,
    token: Option<&str>,
) -> bool {
    let (method, arguments) = match url {
        Some(url) => ("Open", (vec![url.to_owned()], platform_data(token)).to_variant()),
        None => ("Activate", (platform_data(token),).to_variant()),
    };
    connection
        .call_sync(
            Some(app_id),
            &object_path(app_id),
            INTERFACE,
            method,
            Some(&arguments),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            5_000,
            gio::Cancellable::NONE,
        )
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_paths_and_bus_names() {
        assert_eq!(object_path("com.example.Notes"), "/com/example/Notes");
        assert_eq!(object_path("dev.rust-native.App"), "/dev/rust_native/App");
        assert!(is_bus_name("com.example.Notes"));
        assert!(!is_bus_name("Notes"), "a bus name needs two elements");
        assert!(!is_bus_name(":1.42"));
    }

    #[test]
    fn a_second_launch_hands_its_url_to_the_first() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("skipped: no session bus (run under tools/linux-session.sh)");
            return;
        }
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let app_id = format!("dev.rustnative.Test{}", std::process::id());
                let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
                let log = std::rc::Rc::clone(&heard);
                let Claim::First(instance) =
                    claim(&app_id, move |activation| log.borrow_mut().push(activation))
                else {
                    panic!("the first claim wins");
                };
                // The second launch runs elsewhere; here, on another thread
                // while this one serves.
                let id = app_id.clone();
                let second = std::thread::spawn(move || {
                    let context = glib::MainContext::new();
                    context
                        .with_thread_default(|| {
                            // A connection of its own, as another process has.
                            let address = gio::dbus_address_get_for_bus_sync(
                                gio::BusType::Session,
                                gio::Cancellable::NONE,
                            )
                            .expect("the session bus address");
                            let connection = gio::DBusConnection::for_address_sync(
                                &address,
                                gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                                None,
                                gio::Cancellable::NONE,
                            )
                            .expect("a second connection");
                            let claimed =
                                matches!(claim_on(&connection, &id, |_| {}), Claim::AlreadyRunning);
                            let opened = forward_on(
                                &connection,
                                &id,
                                Some("notes://open/7"),
                                Some("token-1"),
                            );
                            let activated = forward_on(&connection, &id, None, None);
                            (claimed, opened, activated)
                        })
                        .expect("a free context")
                });
                let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while heard.borrow().len() < 2 {
                    assert!(std::time::Instant::now() < until, "the launches were not heard");
                    if !context.iteration(false) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
                assert_eq!(second.join().expect("the second launch"), (true, true, true));
                assert_eq!(
                    *heard.borrow(),
                    [
                        Activation {
                            url: Some("notes://open/7".into()),
                            token: Some("token-1".into())
                        },
                        Activation { url: None, token: None },
                    ]
                );
                drop(instance);
                assert!(!forward(&app_id, None, None), "released with the instance");
                assert!(matches!(claim("NotABusName", |_| {}), Claim::Unavailable));
            })
            .expect("a free context");
    }
}
