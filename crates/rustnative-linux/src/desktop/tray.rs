//! The tray icon: a StatusNotifierItem (the freedesktop successor to the
//! XEmbed system tray, shown by Plasma, GNOME with the AppIndicator
//! extension Ubuntu ships, xfce4-panel, waybar, and others) with its menu
//! exported over `com.canonical.dbusmenu`.
//!
//! The item is registered with the `StatusNotifierWatcher`; a desktop with
//! no watcher shows no tray, and the capability is not advertised there.
//! A click on the icon arrives as the `Activate` method, a menu choice as
//! the menu's `Event` method with `clicked`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gio::prelude::*;

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
const MENU_PATH: &str = "/StatusNotifierItem/Menu";

const ITEM_XML: &str = r#"<node><interface name="org.kde.StatusNotifierItem">
  <property name="Category" type="s" access="read"/>
  <property name="Id" type="s" access="read"/>
  <property name="Title" type="s" access="read"/>
  <property name="Status" type="s" access="read"/>
  <property name="IconName" type="s" access="read"/>
  <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
  <property name="ItemIsMenu" type="b" access="read"/>
  <property name="Menu" type="o" access="read"/>
  <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
  <method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
  <method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
  <method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
  <signal name="NewTitle"/>
  <signal name="NewToolTip"/>
  <signal name="NewStatus"><arg type="s"/></signal>
</interface></node>"#;

const MENU_XML: &str = r#"<node><interface name="com.canonical.dbusmenu">
  <property name="Version" type="u" access="read"/>
  <property name="TextDirection" type="s" access="read"/>
  <property name="Status" type="s" access="read"/>
  <property name="IconThemePath" type="as" access="read"/>
  <method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/>
    <arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
  <method name="GetGroupProperties"><arg type="ai" direction="in"/><arg type="as" direction="in"/><arg type="a(ia{sv})" direction="out"/></method>
  <method name="GetProperty"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
  <method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method>
  <method name="EventGroup"><arg type="a(isvu)" direction="in"/><arg type="ai" direction="out"/></method>
  <method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
  <method name="AboutToShowGroup"><arg type="ai" direction="in"/><arg type="ai" direction="out"/><arg type="ai" direction="out"/></method>
  <signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
  <signal name="ItemsPropertiesUpdated"><arg type="a(ia{sv})"/><arg type="a(ias)"/></signal>
</interface></node>"#;

/// Whether a tray host (a `StatusNotifierWatcher`) runs on the session bus.
#[must_use]
pub fn available() -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    connection
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(WATCHER,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1_000,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|reply| reply.child_value(0).get::<bool>())
        .unwrap_or(false)
}

/// What the icon shows.
#[derive(Debug, Default)]
struct State {
    id: String,
    tooltip: String,
    icon: String,
    /// The menu: (id, label) per entry; dbusmenu ids are index + 1.
    menu: Vec<(String, String)>,
    revision: u32,
}

/// A shown tray icon; dropping it removes it.
pub struct Tray {
    connection: gio::DBusConnection,
    state: Rc<RefCell<State>>,
    registrations: Vec<gio::RegistrationId>,
    name: String,
}

impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tray").field("name", &self.name).finish_non_exhaustive()
    }
}

fn menu_item(
    id: i32,
    properties: HashMap<String, glib::Variant>,
    children: Vec<glib::Variant>,
) -> glib::Variant {
    (id, properties, children).to_variant()
}

fn layout(state: &State) -> glib::Variant {
    let children = state
        .menu
        .iter()
        .enumerate()
        .map(|(index, (_, label))| {
            let properties = HashMap::from([
                ("label".to_owned(), label.to_variant()),
                ("enabled".to_owned(), true.to_variant()),
            ]);
            glib::Variant::from_variant(&menu_item(
                i32::try_from(index + 1).unwrap_or(i32::MAX),
                properties,
                Vec::new(),
            ))
        })
        .collect();
    let root = HashMap::from([("children-display".to_owned(), "submenu".to_variant())]);
    menu_item(0, root, children)
}

impl Tray {
    /// Shows a tray icon for application `id` (icon `icon`, a theme icon
    /// name) with `tooltip` and `menu`; `action` hears `"activate"` for a
    /// click on the icon and an item's id for a menu choice. Runs on the
    /// thread whose context delivers `action`.
    ///
    /// # Errors
    ///
    /// No session bus, or no tray host accepted the icon.
    pub fn show(
        id: &str,
        icon: &str,
        tooltip: &str,
        menu: Vec<(String, String)>,
        action: impl Fn(String) + 'static,
    ) -> Result<Self, String> {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .map_err(|error| error.to_string())?;
        let state = Rc::new(RefCell::new(State {
            id: id.to_owned(),
            tooltip: tooltip.to_owned(),
            icon: icon.to_owned(),
            menu,
            revision: 1,
        }));
        let action: Rc<dyn Fn(String)> = Rc::new(action);
        let mut registrations = Vec::new();

        let item = gio::DBusNodeInfo::for_xml(ITEM_XML).map_err(|error| error.to_string())?;
        let item = item.lookup_interface(ITEM_INTERFACE).ok_or("the item interface")?;
        let (properties, activate) = (Rc::clone(&state), Rc::clone(&action));
        registrations.push(
            connection
                .register_object(ITEM_PATH, &item)
                .method_call(move |_, _, _, _, method, _, invocation| {
                    if method == "Activate" || method == "SecondaryActivate" {
                        activate(rustnative_core::surfaces::ACTIVATE.to_owned());
                    }
                    invocation.return_value(None);
                })
                .property(move |_, _, _, _, property| {
                    let state = properties.borrow();
                    match property {
                        "Category" => "ApplicationStatus".to_variant(),
                        "Id" => state.id.to_variant(),
                        "Title" => state.tooltip.to_variant(),
                        "Status" => "Active".to_variant(),
                        "IconName" => state.icon.to_variant(),
                        "ToolTip" => (
                            String::new(),
                            Vec::<(i32, i32, Vec<u8>)>::new(),
                            state.tooltip.clone(),
                            String::new(),
                        )
                            .to_variant(),
                        "ItemIsMenu" => false.to_variant(),
                        "Menu" => glib::variant::ObjectPath::try_from(MENU_PATH)
                            .map_or_else(|_| "/".to_variant(), |path| path.to_variant()),
                        _ => ().to_variant(),
                    }
                })
                .build()
                .map_err(|error| error.to_string())?,
        );

        let menu = gio::DBusNodeInfo::for_xml(MENU_XML).map_err(|error| error.to_string())?;
        let menu = menu.lookup_interface(MENU_INTERFACE).ok_or("the menu interface")?;
        let (calls, chosen) = (Rc::clone(&state), Rc::clone(&action));
        registrations.push(
            connection
                .register_object(MENU_PATH, &menu)
                .method_call(move |_, _, _, _, method, parameters, invocation| {
                    let state = calls.borrow();
                    let reply = match method {
                        "GetLayout" => Some(glib::Variant::tuple_from_iter([
                            state.revision.to_variant(),
                            layout(&state),
                        ])),
                        "GetGroupProperties" => {
                            let ids =
                                parameters.child_value(0).get::<Vec<i32>>().unwrap_or_default();
                            let groups: Vec<(i32, HashMap<String, glib::Variant>)> = ids
                                .into_iter()
                                .filter_map(|id| {
                                    let (_, label) = state
                                        .menu
                                        .get(usize::try_from(id).ok()?.checked_sub(1)?)?;
                                    Some((
                                        id,
                                        HashMap::from([("label".to_owned(), label.to_variant())]),
                                    ))
                                })
                                .collect();
                            Some((groups,).to_variant())
                        }
                        "GetProperty" => {
                            Some((glib::Variant::from_variant(&"".to_variant()),).to_variant())
                        }
                        "Event" => {
                            let (id, event) = (
                                parameters.child_value(0).get::<i32>(),
                                parameters.child_value(1).get::<String>(),
                            );
                            if event.as_deref() == Some("clicked") {
                                let chosen_id = id
                                    .and_then(|id| usize::try_from(id).ok()?.checked_sub(1))
                                    .and_then(|index| state.menu.get(index))
                                    .map(|(id, _)| id.clone());
                                if let Some(item) = chosen_id {
                                    // Delivered after the call returns, with the
                                    // state no longer borrowed.
                                    let chosen = Rc::clone(&chosen);
                                    glib::MainContext::ref_thread_default()
                                        .invoke_local(move || chosen(item));
                                }
                            }
                            None
                        }
                        "EventGroup" => Some((Vec::<i32>::new(),).to_variant()),
                        "AboutToShow" => Some((false,).to_variant()),
                        "AboutToShowGroup" => {
                            Some((Vec::<i32>::new(), Vec::<i32>::new()).to_variant())
                        }
                        _ => None,
                    };
                    invocation.return_value(reply.as_ref());
                })
                .property(|_, _, _, _, property| match property {
                    "Version" => 3_u32.to_variant(),
                    "TextDirection" => "ltr".to_variant(),
                    "Status" => "normal".to_variant(),
                    _ => Vec::<String>::new().to_variant(),
                })
                .build()
                .map_err(|error| error.to_string())?,
        );

        let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        let owned = connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&(name.as_str(), 4_u32).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                2_000,
                gio::Cancellable::NONE,
            )
            .ok()
            .and_then(|reply| reply.child_value(0).get::<u32>());
        let tray = Self { connection, state, registrations, name };
        if !matches!(owned, Some(1 | 4)) {
            return Err("the tray item's bus name is taken".to_owned());
        }
        tray.connection
            .call_sync(
                Some(WATCHER),
                WATCHER_PATH,
                WATCHER,
                "RegisterStatusNotifierItem",
                Some(&(tray.name.as_str(),).to_variant()),
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                2_000,
                gio::Cancellable::NONE,
            )
            .map_err(|error| format!("no tray host accepted the icon: {error}"))?;
        Ok(tray)
    }

    /// Updates the tooltip and menu.
    pub fn update(&self, tooltip: &str, menu: Vec<(String, String)>) {
        let revision = {
            let mut state = self.state.borrow_mut();
            tooltip.clone_into(&mut state.tooltip);
            state.menu = menu;
            state.revision += 1;
            state.revision
        };
        for (path, interface, signal, parameters) in [
            (ITEM_PATH, ITEM_INTERFACE, "NewToolTip", None),
            (ITEM_PATH, ITEM_INTERFACE, "NewTitle", None),
            (MENU_PATH, MENU_INTERFACE, "LayoutUpdated", Some((revision, 0_i32).to_variant())),
        ] {
            let _ = self.connection.emit_signal(None, path, interface, signal, parameters.as_ref());
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // Releasing the name is what tells the host the item is gone.
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
        for registration in self.registrations.drain(..) {
            let _ = self.connection.unregister_object(registration);
        }
    }
}

/// Progress on the application's launcher icon, through the Unity
/// `LauncherEntry` API, which Plasma's task manager, Ubuntu's dock, and
/// Dash to Dock show; `None` hides it. `app_id` names the application's
/// desktop entry (`<app_id>.desktop`).
pub fn set_progress(app_id: &str, progress: Option<f32>) {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return;
    };
    let mut properties: HashMap<String, glib::Variant> = HashMap::new();
    properties.insert("progress".into(), f64::from(progress.unwrap_or(0.0)).to_variant());
    properties.insert("progress-visible".into(), progress.is_some().to_variant());
    let uri = format!("application://{app_id}.desktop");
    let path = format!(
        "/com/canonical/unity/launcherentry/{}",
        glib::compute_checksum_for_string(glib::ChecksumType::Md5, &uri).unwrap_or_default()
    );
    let _ = connection.emit_signal(
        None,
        &path,
        "com.canonical.Unity.LauncherEntry",
        "Update",
        Some(&(uri, properties).to_variant()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::fake_bus::{FakeService, Object};

    #[test]
    fn the_icon_registers_and_its_menu_and_clicks_come_back() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("skipped: no session bus (run under tools/linux-session.sh)");
            return;
        }
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let xml = r#"<node><interface name="org.kde.StatusNotifierWatcher">
                    <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
                    </interface></node>"#;
                let watcher = FakeService::start(
                    WATCHER,
                    vec![Object { path: WATCHER_PATH.into(), xml: xml.into(), handler: std::sync::Arc::new(|_, _| Ok(().to_variant())) }],
                );
                assert!(available());
                let heard = Rc::new(RefCell::new(Vec::new()));
                let log = Rc::clone(&heard);
                let tray = Tray::show(
                    "notes",
                    "accessories-text-editor",
                    "Notes",
                    vec![("new".into(), "New note".into()), ("quit".into(), "Quit".into())],
                    move |action| log.borrow_mut().push(action),
                )
                .expect("shown");
                let registered = watcher.calls_to("RegisterStatusNotifierItem");
                assert_eq!(registered[0].parameters.get::<(String,)>().map(|(name,)| name), Some(tray.name.clone()));

                // The host's side, from a connection of its own.
                let address = gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE).expect("an address");
                let host = gio::DBusConnection::for_address_sync(
                    &address,
                    gio::DBusConnectionFlags::AUTHENTICATION_CLIENT | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                    None,
                    gio::Cancellable::NONE,
                )
                .expect("a host connection");
                let name = tray.name.clone();
                let ask = move |path: &str, interface: &str, method: &str, arguments: glib::Variant| {
                    let (host, name, path, interface, method) =
                        (host.clone(), name.clone(), path.to_owned(), interface.to_owned(), method.to_owned());
                    // Called from another thread so this one can serve it.
                    std::thread::spawn(move || {
                        host.call_sync(Some(&name), &path, &interface, &method, Some(&arguments), None, gio::DBusCallFlags::NONE, 5_000, gio::Cancellable::NONE)
                            .map_err(|error| error.to_string())
                    })
                };
                let wait = |call: std::thread::JoinHandle<Result<glib::Variant, String>>| {
                    while !call.is_finished() {
                        context.iteration(false);
                    }
                    call.join().expect("the call").expect("answered")
                };
                let tooltip = wait(ask(ITEM_PATH, "org.freedesktop.DBus.Properties", "Get", (ITEM_INTERFACE, "ToolTip").to_variant()));
                assert!(format!("{tooltip:?}").contains("Notes"), "{tooltip:?}");
                let menu_layout = wait(ask(MENU_PATH, MENU_INTERFACE, "GetLayout", (0_i32, -1_i32, Vec::<String>::new()).to_variant()));
                assert!(format!("{menu_layout:?}").contains("New note"), "{menu_layout:?}");
                wait(ask(ITEM_PATH, ITEM_INTERFACE, "Activate", (0_i32, 0_i32).to_variant()));
                wait(ask(MENU_PATH, MENU_INTERFACE, "Event", (2_i32, "clicked", glib::Variant::from_variant(&0_i32.to_variant()), 0_u32).to_variant()));
                let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while heard.borrow().len() < 2 {
                    assert!(std::time::Instant::now() < until, "the actions were not heard");
                    if !context.iteration(false) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
                assert_eq!(*heard.borrow(), ["activate", "quit"]);
                tray.update("Notes (2)", vec![("new".into(), "New note".into())]);
                let updated = wait(ask(MENU_PATH, MENU_INTERFACE, "GetLayout", (0_i32, -1_i32, Vec::<String>::new()).to_variant()));
                assert!(!format!("{updated:?}").contains("Quit"), "{updated:?}");
                set_progress("dev.rustnative.Test", Some(0.5));
            })
            .expect("a free context");
    }
}
