//! The XDG desktop portals' request protocol: a portal method that needs
//! the person's answer returns a `Request` object, and the answer arrives
//! later as that object's `Response` signal.
//!
//! The request's path is predictable from the caller's unique name and a
//! `handle_token` it chooses, so the signal is subscribed to before the
//! call — the answer can never arrive unheard.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// The portal service.
pub const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// Its object.
pub const PATH: &str = "/org/freedesktop/portal/desktop";

/// How the person answered a request.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Granted, with the portal's results.
    Granted(HashMap<String, glib::Variant>),
    /// The person refused.
    Denied,
    /// The request ended some other way (closed, failed).
    Ended,
}

/// Whether this process runs inside a sandbox the portals mediate
/// (Flatpak, Snap).
#[must_use]
pub fn sandboxed() -> bool {
    std::path::Path::new("/.flatpak-info").exists() || std::env::var_os("SNAP").is_some()
}

/// A fresh `handle_token`.
fn token() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("rustnative{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The request path a portal will use for `token` from `connection`.
#[must_use]
pub fn request_path(connection: &gio::DBusConnection, token: &str) -> String {
    let sender = connection
        .unique_name()
        .map(|name| name.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    format!("{PATH}/request/{sender}/{token}")
}

/// Calls `interface.method` on the portal with `arguments(token)` (whose
/// options must carry `handle_token`) and awaits the person's answer. Runs
/// on a thread whose thread-default main context is iterating.
///
/// # Errors
///
/// No session bus, or the portal refused the call.
pub async fn request(
    interface: &str,
    method: &str,
    arguments: impl FnOnce(&str) -> glib::Variant,
) -> Result<Response, String> {
    let connection =
        gio::bus_get_future(gio::BusType::Session).await.map_err(|error| error.to_string())?;
    let token = token();
    let path = request_path(&connection, &token);
    let (answer, answered) = tokio::sync::oneshot::channel();
    let answer = std::cell::RefCell::new(Some(answer));
    let _subscription = connection.subscribe_to_signal(
        Some(PORTAL),
        Some("org.freedesktop.portal.Request"),
        Some("Response"),
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let (code, results) = signal
                .parameters
                .get::<(u32, HashMap<String, glib::Variant>)>()
                .unwrap_or((2, HashMap::new()));
            if let Some(answer) = answer.borrow_mut().take() {
                let _ = answer.send(match code {
                    0 => Response::Granted(results),
                    1 => Response::Denied,
                    _ => Response::Ended,
                });
            }
        },
    );
    connection
        .call_future(
            Some(PORTAL),
            PATH,
            interface,
            method,
            Some(&arguments(&token)),
            None,
            gio::DBusCallFlags::NONE,
            10_000,
        )
        .await
        .map_err(|error| format!("the portal refused {interface}.{method}: {error}"))?;
    answered.await.map_err(|_| "the portal's answer was lost".to_owned())
}
