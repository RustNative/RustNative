//! Opening a URL with the person's chosen handler: the OpenURI portal
//! (`org.freedesktop.portal.OpenURI`) where one is on the bus — the only
//! way out of a sandbox, and what a desktop with portals routes through —
//! and GIO's default handler for the URL's type otherwise.

use std::collections::HashMap;

use gio::prelude::*;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// How a URL was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// Through the OpenURI portal.
    Portal,
    /// Through GIO's default handler.
    DefaultHandler,
}

/// Opens `url`.
///
/// # Errors
///
/// Neither the portal nor a default handler could open it.
pub async fn open(url: &str) -> Result<Opened, String> {
    if let Ok(connection) = gio::bus_get_future(gio::BusType::Session).await {
        let options: HashMap<String, glib::Variant> = HashMap::new();
        let opened = connection
            .call_future(
                Some(PORTAL),
                PATH,
                "org.freedesktop.portal.OpenURI",
                "OpenURI",
                Some(&("", url, options).to_variant()),
                None,
                // A portal that is not running is not started for this: the
                // default handler below is the answer then.
                gio::DBusCallFlags::NO_AUTO_START,
                10_000,
            )
            .await;
        if opened.is_ok() {
            return Ok(Opened::Portal);
        }
    }
    gio::AppInfo::launch_default_for_uri_future(url, gio::AppLaunchContext::NONE)
        .await
        .map(|()| Opened::DefaultHandler)
        .map_err(|error| format!("no handler opened {url}: {error}"))
}
