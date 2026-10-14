//! The Web backend's capability answer, on the portable
//! [`Capability`] enum every backend answers (`docs/conformance/new-backend-checklist.md`,
//! rows 1 and 15).
//!
//! Two answers, because a page is rendered before any browser is known:
//!
//! - [`capabilities`] — what the Web backend *realizes* in a browser that
//!   offers it. A page render puts it in the tree's environment
//!   (`keys::CAPABILITIES`), so a component rendered on the server asks the
//!   same question it asks on a native host.
//! - [`from_effects`] — what *this* browser has, from the effect names the
//!   runtime's `caps()` (and a client component's `fx.capabilities`) reports.
//!
//! Surfaces beyond the page — a tray, a jump list, taskbar progress, widgets,
//! tiles, live activities, extensions, instant apps, companion devices — have
//! no browser equivalent and are answered no.

use rustnative_core::{Capability, PlatformCapabilities};

/// Each runtime effect, and the capability it realizes.
const EFFECTS: &[(&str, Capability)] = &[
    ("copy", Capability::Clipboard),
    ("read_clipboard", Capability::Clipboard),
    ("notify", Capability::Notifications),
    ("store", Capability::Storage),
    ("load", Capability::Storage),
    ("db_put", Capability::IndexedStorage),
    ("cache_put", Capability::OfflineCache),
    ("permission", Capability::Permissions),
    ("share", Capability::SystemShare),
    ("locate", Capability::Location),
    ("open_file", Capability::FileDialogs),
    ("save_file", Capability::FileDialogs),
    ("download", Capability::Downloads),
    ("socket_open", Capability::WebSocket),
    ("worker", Capability::BackgroundWorkers),
    ("media", Capability::Camera),
    ("media", Capability::Microphone),
    ("bluetooth", Capability::Bluetooth),
    ("sensor", Capability::Sensors),
    ("history", Capability::History),
    ("service_worker", Capability::ServiceWorker),
];

/// What every browser the runtime supports realizes, with no effect to ask
/// about: the document and its input.
const DOCUMENT: &[Capability] = &[
    Capability::Http,
    Capability::UrlLaunch,
    Capability::DeepLinks,
    Capability::Lifecycle,
    Capability::Hover,
    Capability::Cursors,
    Capability::Touch,
    Capability::Pen,
    Capability::Ime,
    Capability::RightToLeft,
    Capability::HostTraits,
    Capability::CustomDrawing,
    Capability::WebContent,
];

/// What the Web backend realizes in a browser that offers it.
#[must_use]
pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities::new(
        DOCUMENT.iter().copied().chain(EFFECTS.iter().map(|(_, capability)| *capability)),
    )
}

/// What a browser has, from the effect names its runtime reports
/// (`rn.caps()`, `fx.capabilities`).
#[must_use]
pub fn from_effects<'a>(names: impl IntoIterator<Item = &'a str>) -> PlatformCapabilities {
    let names: Vec<&str> = names.into_iter().collect();
    PlatformCapabilities::new(
        DOCUMENT.iter().copied().chain(
            EFFECTS
                .iter()
                .filter(|(effect, _)| names.contains(effect))
                .map(|(_, capability)| *capability),
        ),
    )
}

/// Where the browser runtime's inspection table is served, below the
/// framework's asset path (`/_rn/inspect.json`).
pub const INSPECTION_TABLE: &str = "inspect.json";

/// What the browser runtime answers the inspection protocol's capability
/// question from (`rn.inspect`, Milestone 44): the effect-to-capability
/// map, the document's own capabilities, every capability's name, and the
/// Web's style table and unit mapping — the same rows a native backend's
/// inspection server reports.
#[must_use]
pub fn inspection_table() -> serde_json::Value {
    use rustnative_style::StyleSupport;
    let name = |capability: &Capability| format!("{capability:?}");
    let units = rustnative_style::WEB_UNITS;
    serde_json::json!({
        "backend": "web",
        "effects": EFFECTS.iter().map(|(effect, capability)| (effect, name(capability))).collect::<Vec<_>>(),
        "document": DOCUMENT.iter().map(name).collect::<Vec<_>>(),
        "all": Capability::ALL.iter().map(name).collect::<Vec<_>>(),
        "style": rustnative_style::WEB.rows().map(|(property, support)| serde_json::json!({
            "what": property.to_string(),
            "why": match support {
                StyleSupport::Realized => "realized".to_owned(),
                StyleSupport::Approximated(how) => format!("approximated: {how}"),
                StyleSupport::Unavailable(why) => format!("unavailable: {why}"),
            },
        })).collect::<Vec<_>>(),
        "units": {
            "host_unit": units.host_unit,
            "pixel": units.pixel,
            "rem": units.rem,
            "rounding": units.rounding,
        },
    })
}

#[cfg(test)]
mod tests {
    use rustnative_core::SurfaceKind;

    use super::*;

    #[test]
    fn surfaces_beyond_the_page_are_answered_no() {
        let web = capabilities();
        for kind in [
            SurfaceKind::TrayExtra,
            SurfaceKind::JumpList,
            SurfaceKind::TaskbarProgress,
            SurfaceKind::Widget,
            SurfaceKind::Tile,
            SurfaceKind::LiveActivity,
            SurfaceKind::Extension,
            SurfaceKind::InstantApp,
            SurfaceKind::CompanionDevice,
        ] {
            assert!(!web.supports(Capability::Surface(kind)), "{kind:?}");
        }
        assert!(!web.supports(Capability::Menus), "no menu bar of the application's own");
        assert!(web.supports(Capability::Http) && web.supports(Capability::SystemShare));
    }

    #[test]
    fn a_browser_answers_only_what_it_reports() {
        let browser = from_effects(["copy", "socket_open"]);
        assert!(browser.supports(Capability::Clipboard));
        assert!(browser.supports(Capability::WebSocket));
        assert!(!browser.supports(Capability::SystemShare), "no `share` reported");
        assert!(browser.supports(Capability::Http), "the document's own");
    }

    #[test]
    fn the_inspection_table_holds_every_row() {
        let table = inspection_table();
        assert_eq!(table["backend"], "web");
        assert_eq!(table["all"].as_array().map(Vec::len), Some(Capability::ALL.len()));
        assert!(table["style"].as_array().is_some_and(|rows| !rows.is_empty()));
        assert_eq!(table["units"]["host_unit"], rustnative_style::WEB_UNITS.host_unit);
    }
}
