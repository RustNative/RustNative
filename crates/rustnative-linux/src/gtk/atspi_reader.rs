//! A minimal AT-SPI2 client, for tests to read the application's
//! accessibility tree the way a screen reader does: over the accessibility
//! bus, from another connection.
//!
//! It runs on a thread of its own (the application answers AT-SPI calls on
//! the GTK thread's main context, which must keep iterating while the
//! reader waits), and returns plain data.
#![allow(
    clippy::expect_used,
    reason = "test support: an accessibility bus that cannot be reached is a failed test"
)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use gio::prelude::*;

/// One accessible, as AT-SPI2 reports it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Accessible {
    pub(crate) bus: String,
    pub(crate) path: String,
    pub(crate) role: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) id: String,
    /// AT-SPI state bits (`AtspiStateType` positions).
    pub(crate) states: Vec<u32>,
    /// `(relation type, target paths)`.
    pub(crate) relations: Vec<(u32, Vec<String>)>,
    pub(crate) children: Vec<Accessible>,
}

/// `ATSPI_STATE_CHECKED`.
pub(crate) const STATE_CHECKED: u32 = 4;
/// `ATSPI_STATE_REQUIRED`.
pub(crate) const STATE_REQUIRED: u32 = 33;
/// `ATSPI_STATE_INDETERMINATE` (a mixed check state).
pub(crate) const STATE_INDETERMINATE: u32 = 32;
/// `ATSPI_RELATION_LABELLED_BY`.
pub(crate) const RELATION_LABELLED_BY: u32 = 2;

impl Accessible {
    /// Whether state `bit` is set.
    pub(crate) fn has_state(&self, bit: u32) -> bool {
        let word = usize::try_from(bit / 32).unwrap_or(0);
        self.states.get(word).is_some_and(|value| value & (1 << (bit % 32)) != 0)
    }

    /// The first accessible, in preorder, satisfying `wanted`.
    pub(crate) fn find(&self, wanted: &dyn Fn(&Self) -> bool) -> Option<&Self> {
        if wanted(self) {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(wanted))
    }

    /// The first accessible named `name`.
    pub(crate) fn named(&self, name: &str) -> Option<&Self> {
        self.find(&|accessible| accessible.name == name)
    }
}

/// A connection to the accessibility bus.
pub(crate) struct Client {
    connection: gio::DBusConnection,
}

impl Client {
    fn connect() -> Self {
        let address =
            gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE)
                .expect("a session bus");
        let flags = gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION;
        let session =
            gio::DBusConnection::for_address_sync(&address, flags, None, gio::Cancellable::NONE)
                .expect("the session bus");
        let reply = session
            .call_sync(
                Some("org.a11y.Bus"),
                "/org/a11y/bus",
                "org.a11y.Bus",
                "GetAddress",
                None,
                None,
                gio::DBusCallFlags::NONE,
                5_000,
                gio::Cancellable::NONE,
            )
            .expect("the accessibility bus launcher");
        let a11y = reply.child_value(0).get::<String>().expect("an address");
        let connection =
            gio::DBusConnection::for_address_sync(&a11y, flags, None, gio::Cancellable::NONE)
                .expect("the accessibility bus");
        Self { connection }
    }

    fn call(
        &self,
        bus: &str,
        path: &str,
        interface: &str,
        method: &str,
        args: Option<&glib::Variant>,
    ) -> Option<glib::Variant> {
        self.connection
            .call_sync(
                Some(bus),
                path,
                interface,
                method,
                args,
                None,
                gio::DBusCallFlags::NONE,
                5_000,
                gio::Cancellable::NONE,
            )
            .ok()
    }

    fn property(
        &self,
        bus: &str,
        path: &str,
        interface: &str,
        name: &str,
    ) -> Option<glib::Variant> {
        self.call(
            bus,
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(interface, name).to_variant()),
        )?
        .child_value(0)
        .as_variant()
    }

    fn string(&self, bus: &str, path: &str, name: &str) -> String {
        self.property(bus, path, "org.a11y.atspi.Accessible", name)
            .and_then(|value| value.get::<String>())
            .unwrap_or_default()
    }

    fn children(&self, bus: &str, path: &str) -> Vec<(String, String)> {
        self.call(bus, path, "org.a11y.atspi.Accessible", "GetChildren", None)
            .and_then(|reply| {
                reply.child_value(0).get::<Vec<(String, glib::variant::ObjectPath)>>()
            })
            .map(|children| {
                children.into_iter().map(|(bus, path)| (bus, path.to_string())).collect()
            })
            .unwrap_or_default()
    }

    fn read(&self, bus: &str, path: &str, depth: usize) -> Accessible {
        let role = self
            .call(bus, path, "org.a11y.atspi.Accessible", "GetRoleName", None)
            .and_then(|reply| reply.child_value(0).get::<String>())
            .unwrap_or_default();
        let states = self
            .call(bus, path, "org.a11y.atspi.Accessible", "GetState", None)
            .and_then(|reply| reply.child_value(0).get::<Vec<u32>>())
            .unwrap_or_default();
        let relations = self
            .call(bus, path, "org.a11y.atspi.Accessible", "GetRelationSet", None)
            .and_then(|reply| {
                reply.child_value(0).get::<Vec<(u32, Vec<(String, glib::variant::ObjectPath)>)>>()
            })
            .map(|relations| {
                relations
                    .into_iter()
                    .map(|(kind, targets)| {
                        (kind, targets.into_iter().map(|(_, path)| path.to_string()).collect())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let children = if depth > 40 {
            Vec::new()
        } else {
            self.children(bus, path)
                .into_iter()
                .map(|(bus, path)| self.read(&bus, &path, depth + 1))
                .collect()
        };
        Accessible {
            bus: bus.to_owned(),
            path: path.to_owned(),
            role,
            name: self.string(bus, path, "Name"),
            description: self.string(bus, path, "Description"),
            id: self.string(bus, path, "AccessibleId"),
            states,
            relations,
            children,
        }
    }

    /// Every application's tree on the bus.
    fn applications(&self) -> Vec<Accessible> {
        self.children("org.a11y.atspi.Registry", "/org/a11y/atspi/accessible/root")
            .into_iter()
            .map(|(bus, path)| self.read(&bus, &path, 0))
            .collect()
    }

    /// A range accessible's current value.
    fn value(&self, bus: &str, path: &str) -> Option<f64> {
        self.property(bus, path, "org.a11y.atspi.Value", "CurrentValue")?.get::<f64>()
    }

    /// Sets a range accessible's value, as a screen reader does.
    fn set_value(&self, bus: &str, path: &str, value: f64) -> bool {
        let arguments = glib::Variant::tuple_from_iter([
            "org.a11y.atspi.Value".to_variant(),
            "CurrentValue".to_variant(),
            glib::Variant::from_variant(&value.to_variant()),
        ]);
        let reply = self.connection.call_sync(
            Some(bus),
            path,
            "org.freedesktop.DBus.Properties",
            "Set",
            Some(&arguments),
            None,
            gio::DBusCallFlags::NONE,
            5_000,
            gio::Cancellable::NONE,
        );
        if let Err(error) = &reply {
            eprintln!("Value.CurrentValue refused: {error}");
        }
        reply.is_ok()
    }

    /// An accessible's actions' names.
    fn actions(&self, bus: &str, path: &str) -> Vec<String> {
        self.call(bus, path, "org.a11y.atspi.Action", "GetActions", None)
            .and_then(|reply| reply.child_value(0).get::<Vec<(String, String, String)>>())
            .map(|actions| actions.into_iter().map(|(name, _, _)| name).collect())
            .unwrap_or_default()
    }

    /// Performs action `index`.
    fn do_action(&self, bus: &str, path: &str, index: i32) -> bool {
        self.call(bus, path, "org.a11y.atspi.Action", "DoAction", Some(&(index,).to_variant()))
            .and_then(|reply| reply.child_value(0).get::<bool>())
            .unwrap_or(false)
    }
}

/// A request the reader thread carries out.
pub(crate) enum Request {
    /// Read every application's tree.
    Tree,
    /// Read a range accessible's value.
    Value { bus: String, path: String },
    /// Set a range accessible's value.
    SetValue { bus: String, path: String, value: f64 },
    /// List an accessible's actions.
    Actions { bus: String, path: String },
    /// Perform an action.
    DoAction { bus: String, path: String, index: i32 },
}

/// What a request answered.
#[derive(Debug)]
pub(crate) enum Answer {
    Tree(Vec<Accessible>),
    Value(Option<f64>),
    Done(bool),
    Actions(Vec<String>),
}

/// Carries out `request` on a reader thread while the calling (GTK)
/// thread keeps its main context iterating, so the application can answer.
pub(crate) fn ask(request: Request) -> Answer {
    let (reply, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let client = Client::connect();
        let answered = match request {
            Request::Tree => Answer::Tree(client.applications()),
            Request::Value { bus, path } => Answer::Value(client.value(&bus, &path)),
            Request::SetValue { bus, path, value } => {
                Answer::Done(client.set_value(&bus, &path, value))
            }
            Request::Actions { bus, path } => Answer::Actions(client.actions(&bus, &path)),
            Request::DoAction { bus, path, index } => {
                Answer::Done(client.do_action(&bus, &path, index))
            }
        };
        let _ = reply.send(answered);
    });
    let context = glib::MainContext::default();
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(answered) = answer.try_recv() {
            return answered;
        }
        assert!(Instant::now() < until, "the AT-SPI reader did not answer");
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// The AT-SPI events named `member` (on `org.a11y.atspi.Event.Object`)
/// emitted while `during` runs and shortly after, as their arguments'
/// text.
pub(crate) fn listen(member: &'static str, during: impl FnOnce()) -> Vec<String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let seen = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (ready, subscribed) = mpsc::channel();
    let thread = {
        let (seen, stop) = (Arc::clone(&seen), Arc::clone(&stop));
        std::thread::spawn(move || {
            let context = glib::MainContext::new();
            context
                .with_thread_default(|| {
                    let client = Client::connect();
                    let recorded = Arc::clone(&seen);
                    let _subscription = client.connection.subscribe_to_signal(
                        None,
                        Some("org.a11y.atspi.Event.Object"),
                        Some(member),
                        None,
                        None,
                        gio::DBusSignalFlags::NONE,
                        move |signal| {
                            recorded
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push(signal.parameters.print(false).to_string());
                        },
                    );
                    let _ = ready.send(());
                    while !stop.load(Ordering::Acquire) {
                        context.iteration(false);
                        std::thread::sleep(Duration::from_millis(2));
                    }
                })
                .expect("the listener's context is free");
        })
    };
    let context = glib::MainContext::default();
    while subscribed.try_recv().is_err() {
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    during();
    let until = Instant::now() + Duration::from_millis(700);
    while Instant::now() < until {
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    stop.store(true, Ordering::Release);
    let _ = thread.join();

    seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// The accessible window titled `title`, read from the bus.
pub(crate) fn window(title: &str) -> Accessible {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let Answer::Tree(applications) = ask(Request::Tree) else {
            unreachable!("a tree request answers a tree")
        };
        let found = applications.iter().find_map(|application| {
            application.children.iter().find(|window| window.name == title).cloned()
        });
        if let Some(found) = found {
            return found;
        }
        assert!(
            Instant::now() < until,
            "no accessible window titled {title:?} appeared on the bus"
        );
    }
}
