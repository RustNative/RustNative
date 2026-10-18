//! Opening a URL with the person's chosen handler: the OpenURI portal
//! (`org.freedesktop.portal.OpenURI`) where one is on the bus — the only
//! way out of a sandbox, and what a desktop with portals routes through —
//! and GIO's default handler for the URL's type otherwise. Either opens any
//! scheme an installed application claims, so a URL goes to them only once
//! a [`UrlPolicy`] allows it.

use std::collections::HashMap;

use gio::prelude::*;
use rustnative_core::UrlPolicy;

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

/// Opens `url`, if `policy` allows it.
///
/// # Errors
///
/// `policy` refuses `url`, or neither the portal nor a default handler
/// could open it.
pub async fn open(url: &str, policy: &UrlPolicy) -> Result<Opened, String> {
    policy.check(url).map_err(|refused| refused.to_string())?;
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
