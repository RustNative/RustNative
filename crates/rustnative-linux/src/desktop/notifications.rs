//! Desktop notifications over D-Bus.
//!
//! The notification server (`org.freedesktop.Notifications`, which every
//! desktop runs — GNOME Shell, Plasma, xfce4-notifyd, dunst, mako) is used
//! where one owns the name; inside a sandbox, where it is not reachable,
//! the Notification portal is. A click on the notification is reported
//! through `on_click` (the server's `ActionInvoked` for the default action).

use std::collections::HashMap;

use gio::prelude::*;

const SERVER: &str = "org.freedesktop.Notifications";
const SERVER_PATH: &str = "/org/freedesktop/Notifications";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";

/// Where a notification was delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivered {
    /// The notification server, which gave it this id.
    Server(u32),
    /// The Notification portal, under this id.
    Portal(String),
}

async fn has_owner(connection: &gio::DBusConnection, name: &str) -> bool {
    connection
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(name,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1_000,
        )
        .await
        .ok()
        .and_then(|reply| reply.child_value(0).get::<bool>())
        .unwrap_or(false)
}

/// Shows a notification as `application`, calling `on_click` when the
/// person activates it.
///
/// # Errors
///
/// There is no session bus, or neither a notification server nor the
/// portal accepted it.
pub async fn notify(
    application: &str,
    title: &str,
    body: &str,
    on_click: Option<Box<dyn Fn()>>,
) -> Result<Delivered, String> {
    let connection =
        gio::bus_get_future(gio::BusType::Session).await.map_err(|error| error.to_string())?;
    if has_owner(&connection, SERVER).await {
        let actions: Vec<String> =
            if on_click.is_some() { vec!["default".into(), "Open".into()] } else { Vec::new() };
        let hints: HashMap<String, glib::Variant> = HashMap::new();
        let arguments = (application, 0_u32, "", title, body, actions, hints, -1_i32).to_variant();
        let reply = connection
            .call_future(
                Some(SERVER),
                SERVER_PATH,
                SERVER,
                "Notify",
                Some(&arguments),
                None,
                gio::DBusCallFlags::NONE,
                5_000,
            )
            .await
            .map_err(|error| error.to_string())?;
        let id = reply.child_value(0).get::<u32>().ok_or("the server answered without an id")?;
        if let Some(on_click) = on_click {
            // Kept for the life of the connection — a notification can be
            // clicked long after it was shown — so never unsubscribed (a
            // dropped subscription, weak or not, unsubscribes).
            let subscription = connection.subscribe_to_signal(
                Some(SERVER),
                Some(SERVER),
                Some("ActionInvoked"),
                Some(SERVER_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if signal
                        .parameters
                        .get::<(u32, String)>()
                        .is_some_and(|(clicked, action)| clicked == id && action == "default")
                    {
                        on_click();
                    }
                },
            );
            std::mem::forget(subscription);
        }
        return Ok(Delivered::Server(id));
    }
    let id = format!("rustnative-{}", glib::monotonic_time());
    let mut notification: HashMap<String, glib::Variant> = HashMap::new();
    notification.insert("title".into(), title.to_variant());
    notification.insert("body".into(), body.to_variant());
    connection
        .call_future(
            Some(PORTAL),
            PORTAL_PATH,
            "org.freedesktop.portal.Notification",
            "AddNotification",
            Some(&(id.as_str(), notification).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            5_000,
        )
        .await
        .map_err(|error| format!("no notification server, and the portal refused: {error}"))?;
    Ok(Delivered::Portal(id))
}
