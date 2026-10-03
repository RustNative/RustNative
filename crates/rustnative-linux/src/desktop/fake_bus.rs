//! Fake freedesktop services for tests, served on the test's private
//! session bus (`tools/linux-session.sh` runs every test under
//! `dbus-run-session`).
//!
//! A fake runs on a thread of its own, on a connection of its own: the code
//! under test calls it synchronously from the GTK thread, and a service
//! answering on that same thread's main context would never get to run.
//! Each call it receives is recorded, so a test can assert on exactly what
//! the backend sent over the wire.
#![allow(
    clippy::expect_used,
    reason = "test support: a fake service that cannot start is a failed test, said as such"
)]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use gio::prelude::*;

/// A method handler: `(method, parameters) -> reply`, where the reply is
/// the method's out-arguments as a tuple variant (`()` for none), or the
/// message of a D-Bus error.
pub(crate) type Handler =
    Arc<dyn Fn(&str, &glib::Variant) -> Result<glib::Variant, String> + Send + Sync>;

/// One call the fake received.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    /// The object path it was made on.
    pub(crate) path: String,
    /// The method.
    pub(crate) method: String,
    /// Its parameters.
    pub(crate) parameters: glib::Variant,
}

/// An object the fake serves: its path, its interface's introspection XML
/// (`<node><interface name="…">…</interface></node>`), and its handler.
pub(crate) struct Object {
    pub(crate) path: String,
    pub(crate) xml: String,
    pub(crate) handler: Handler,
}

enum Command {
    Emit { path: String, interface: String, signal: String, parameters: glib::Variant },
    Stop,
}

/// A running fake service.
pub(crate) struct FakeService {
    calls: Arc<Mutex<Vec<Call>>>,
    commands: mpsc::Sender<Command>,
    wake: glib::MainContext,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeService {
    /// Serves `objects` under the well-known `name` on the session bus, and
    /// returns once the name is owned.
    ///
    /// # Panics
    ///
    /// When there is no session bus (the test is not running under
    /// `dbus-run-session`) or the name cannot be owned.
    pub(crate) fn start(name: &str, objects: Vec<Object>) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (commands, receive) = mpsc::channel::<Command>();
        let (ready, started) = mpsc::channel::<glib::MainContext>();
        let name = name.to_owned();
        let recorded = Arc::clone(&calls);
        let thread = std::thread::spawn(move || {
            let context = glib::MainContext::new();
            context
                .with_thread_default(|| {
                    let address = gio::dbus_address_get_for_bus_sync(
                        gio::BusType::Session,
                        gio::Cancellable::NONE,
                    )
                    .expect("a session bus: run under dbus-run-session");
                    let connection = gio::DBusConnection::for_address_sync(
                        &address,
                        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                        None,
                        gio::Cancellable::NONE,
                    )
                    .expect("connecting to the session bus");
                    let mut registrations = Vec::new();
                    for object in objects {
                        registrations.push(register(&connection, object, &recorded));
                    }
                    let reply = connection
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
                        .expect("RequestName");
                    assert_eq!(reply.child_value(0).get::<u32>(), Some(1), "owning {name}");
                    ready.send(context.clone()).expect("the test waits for the fake");
                    loop {
                        context.iteration(true);
                        match receive.try_recv() {
                            Ok(Command::Emit { path, interface, signal, parameters }) => {
                                let _ = connection.emit_signal(
                                    None,
                                    &path,
                                    &interface,
                                    &signal,
                                    Some(&parameters),
                                );
                                let _ = connection.flush_sync(gio::Cancellable::NONE);
                            }
                            Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
                            Err(mpsc::TryRecvError::Empty) => {}
                        }
                    }
                    for registration in registrations {
                        let _ = connection.unregister_object(registration);
                    }
                    let _ = connection.close_sync(gio::Cancellable::NONE);
                })
                .expect("the fake's context is free");
        });
        let wake = started.recv().expect("the fake service starts");
        Self { calls, commands, wake, thread: Some(thread) }
    }

    /// Every call received so far.
    pub(crate) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// The calls of `method` received so far.
    pub(crate) fn calls_to(&self, method: &str) -> Vec<Call> {
        self.calls().into_iter().filter(|call| call.method == method).collect()
    }

    /// Emits `signal` from `path`.
    pub(crate) fn emit(
        &self,
        path: &str,
        interface: &str,
        signal: &str,
        parameters: glib::Variant,
    ) {
        self.send(Command::Emit {
            path: path.to_owned(),
            interface: interface.to_owned(),
            signal: signal.to_owned(),
            parameters,
        });
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
        self.wake.wakeup();
    }
}

impl Drop for FakeService {
    fn drop(&mut self) {
        self.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn register(
    connection: &gio::DBusConnection,
    object: Object,
    calls: &Arc<Mutex<Vec<Call>>>,
) -> gio::RegistrationId {
    let node = gio::DBusNodeInfo::for_xml(&object.xml).expect("valid introspection XML");
    let interface = node.interfaces().first().cloned().expect("one interface");
    let handler = object.handler;
    let calls = Arc::clone(calls);
    connection
        .register_object(&object.path, &interface)
        .method_call(move |_, _, path, _, method, parameters, invocation| {
            calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(Call {
                path: path.to_owned(),
                method: method.to_owned(),
                parameters: parameters.clone(),
            });
            match handler(method, &parameters) {
                Ok(reply) => invocation.return_value(Some(&reply)),
                Err(message) => {
                    invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", &message);
                }
            }
        })
        .build()
        .expect("registering the fake object")
}
