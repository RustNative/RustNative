//! What an intent brings the application: a deep link (`ACTION_VIEW` with
//! one of its URL schemes, Milestone 30), content shared to it
//! (`ACTION_SEND`, a share target, Milestone 57), or a tap on one of its
//! surfaces (a widget, a tile, a notification).

use rustnative_core::SharedContent;
use rustnative_core::capability::SurfaceKind;

/// What an intent brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Incoming {
    DeepLink(String),
    Share(SharedContent),
    SurfaceAction(SurfaceKind, String),
    Nothing,
}

/// The surface a tap's `dev.rustnative.surface` extra names.
pub(crate) fn surface_kind(name: &str) -> SurfaceKind {
    match name {
        "widget" => SurfaceKind::Widget,
        "tile" => SurfaceKind::Tile,
        "ongoing" => SurfaceKind::LiveActivity,
        "shortcut" => SurfaceKind::JumpList,
        "extension" => SurfaceKind::Extension,
        // A notification belongs to the tray in the portable model.
        _ => SurfaceKind::TrayExtra,
    }
}

/// What `RnIntents.describe` said.
pub(crate) fn from_description(parts: &[String]) -> Incoming {
    match parts.first().map(String::as_str) {
        Some("view") => {
            parts.get(1).map_or(Incoming::Nothing, |url| Incoming::DeepLink(url.clone()))
        }
        Some("send") => {
            let text = parts.get(1).filter(|text| !text.is_empty()).cloned();
            let subject = parts.get(2).filter(|subject| !subject.is_empty()).cloned();
            let items = parts
                .get(3..)
                .unwrap_or_default()
                .chunks_exact(2)
                .map(|pair| rustnative_core::SharedItem {
                    uri: pair[0].clone(),
                    mime_type: pair[1].clone(),
                })
                .collect();
            Incoming::Share(SharedContent { text, subject, items })
        }
        Some("surface") => {
            let surface = surface_kind(parts.get(1).map_or("", String::as_str));
            Incoming::SurfaceAction(
                surface,
                parts.get(2).cloned().unwrap_or_else(|| "activate".to_owned()),
            )
        }
        _ => Incoming::Nothing,
    }
}

/// Reads `intent`.
#[cfg(target_os = "android")]
pub(crate) fn read(intent: &crate::jni_host::JavaRef) -> Result<Incoming, crate::Error> {
    use crate::jni_host::{Arg, Class, call_static};
    let parts = call_static(
        Class::Intents,
        "describe",
        "(Landroid/content/Intent;)[Ljava/lang/String;",
        &[Arg::Obj(intent)],
    )?
    .strings();
    Ok(from_description(&parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_become_what_the_application_hears() {
        let parts =
            |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            from_description(&parts(&["view", "notes://open/42"])),
            Incoming::DeepLink("notes://open/42".into())
        );
        let Incoming::Share(share) =
            from_description(&parts(&["send", "hello", "", "content://a/1", "image/png"]))
        else {
            panic!("a share")
        };
        assert_eq!(share.text.as_deref(), Some("hello"));
        assert_eq!(share.subject, None);
        assert_eq!(share.items.len(), 1);
        assert_eq!(share.items[0].mime_type, "image/png");
        assert_eq!(
            from_description(&parts(&["surface", "widget", "refresh"])),
            Incoming::SurfaceAction(SurfaceKind::Widget, "refresh".into())
        );
        assert_eq!(from_description(&parts(&[""])), Incoming::Nothing);
    }
}
