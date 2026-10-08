//! Surfaces beyond the main window (Milestone 57): what the application
//! queues on `Services::surfaces()`, realized as Android's own.
//!
//! | Portable | Android |
//! |---|---|
//! | `Notify`, `NotifyWithActions` | a notification on the category's channel; actions return as `Event::SurfaceAction` |
//! | `UpdateWidget` | the home-screen widget declared with that id (`RnWidgetProvider`, `RemoteViews`) |
//! | `UpdateTile` | the quick-settings tile declared with that id (`RnTileService`) |
//! | `Ongoing`, `EndOngoing` | an ongoing notification with progress (`SurfaceKind::LiveActivity`) |
//! | `JumpList` | the launcher's dynamic shortcuts (`ShortcutManager`) |
//! | `ShowTray`, `HideTray`, `Progress` | unavailable: Android has no tray or taskbar; logged once |
//!
//! A widget or tile must be declared in `rustnative.toml`
//! (`[[android.widgets]]`, `[[android.tiles]]`): an update for an undeclared
//! id is logged and dropped.

use std::sync::atomic::{AtomicBool, Ordering};

use rustnative_core::surfaces::SurfaceCommand;

use crate::Error;
use crate::jni_host::{Arg, Class, Ret, call_static};
use crate::registry::WindowRegistry;

/// The notification id of ongoing activity `id`: stable across updates.
pub(crate) fn ongoing_id(id: &str) -> i32 {
    // FNV-1a, folded into the positive half (ids of other notifications
    // count up from 1, so these sit far from them).
    let hash = id
        .bytes()
        .fold(0x811c_9dc5_u32, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193));
    i32::try_from(hash >> 1).unwrap_or(i32::MAX) | 0x4000_0000
}

fn pairs(items: &[rustnative_core::surfaces::TrayMenuItem]) -> Vec<String> {
    items.iter().flat_map(|item| [item.id.clone(), item.label.clone()]).collect()
}

fn reported(ret: Result<Ret, Error>, what: &str) {
    match ret {
        Ok(ret) => {
            if let Some(message) = ret.string() {
                crate::log::error(&format!("{what}: {message}"));
            }
        }
        Err(error) => crate::log::error(&format!("{what}: {error}")),
    }
}

static TRAY_LOGGED: AtomicBool = AtomicBool::new(false);

/// Applies what the application asked of its surfaces.
#[allow(clippy::unnecessary_wraps, reason = "a surface's failure is logged, never the window's")]
pub(crate) fn apply(registry: &mut WindowRegistry) -> Result<(), Error> {
    let commands =
        registry.with_application(|application| application.services().surfaces().take());
    for command in commands {
        apply_one(command);
    }
    Ok(())
}

pub(crate) fn apply_one(command: SurfaceCommand) {
    match command {
        SurfaceCommand::Notify { title, body } => {
            let system = crate::services::AndroidSystem::default();
            if let Err(error) = system.notify_with_actions(&title, &body, &[]) {
                crate::log::error(&format!("a notification: {error}"));
            }
        }
        SurfaceCommand::NotifyWithActions { title, body, actions, category } => {
            let system = crate::services::AndroidSystem::on_channel(category);
            let actions: Vec<(String, String)> =
                actions.into_iter().map(|item| (item.id, item.label)).collect();
            if let Err(error) = system.notify_with_actions(&title, &body, &actions) {
                crate::log::error(&format!("a notification: {error}"));
            }
        }
        SurfaceCommand::UpdateWidget { id, content } => {
            let declared = call_static(
                Class::Services,
                "updateWidget",
                "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;)Z",
                &[
                    Arg::Str(&id),
                    Arg::Str(&content.title),
                    Arg::Strs(&content.lines),
                    Arg::Strs(&pairs(&content.actions)),
                ],
            );
            if !declared.is_ok_and(Ret::bool) {
                crate::log::error(&format!(
                    "no widget `{id}` is declared in rustnative.toml ([[android.widgets]])"
                ));
            }
        }
        SurfaceCommand::UpdateTile { id, state } => {
            let declared = call_static(
                Class::Services,
                "updateTile",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)Z",
                &[
                    Arg::Str(&id),
                    Arg::Str(&state.label),
                    Arg::OptStr(state.subtitle.as_deref()),
                    Arg::Bool(state.active),
                ],
            );
            if !declared.is_ok_and(Ret::bool) {
                crate::log::error(&format!(
                    "no tile `{id}` is declared in rustnative.toml ([[android.tiles]])"
                ));
            }
        }
        SurfaceCommand::Ongoing { id, activity } => {
            #[allow(clippy::cast_possible_truncation, reason = "a percentage")]
            let progress = activity
                .progress
                .map_or(-1, |progress| (progress.clamp(0.0, 1.0) * 100.0).round() as i32);
            reported(
                call_static(
                    Class::Services,
                    "ongoing",
                    "(ILjava/lang/String;Ljava/lang/String;I[Ljava/lang/String;)Ljava/lang/String;",
                    &[
                        Arg::Int(ongoing_id(&id)),
                        Arg::Str(&activity.title),
                        Arg::Str(&activity.body),
                        Arg::Int(progress),
                        Arg::Strs(&pairs(&activity.actions)),
                    ],
                ),
                "an ongoing activity",
            );
        }
        SurfaceCommand::EndOngoing { id } => {
            reported(
                call_static(
                    Class::Services,
                    "cancelNotification",
                    "(I)V",
                    &[Arg::Int(ongoing_id(&id))],
                ),
                "ending an ongoing activity",
            );
        }
        SurfaceCommand::JumpList(tasks) => {
            let labels: Vec<String> = tasks.iter().map(|task| task.label.clone()).collect();
            let arguments: Vec<String> = tasks.into_iter().map(|task| task.arguments).collect();
            reported(
                call_static(
                    Class::Services,
                    "setShortcuts",
                    "([Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/String;",
                    &[Arg::Strs(&labels), Arg::Strs(&arguments)],
                ),
                "the launcher shortcuts",
            );
        }
        _ => {
            if !TRAY_LOGGED.swap(true, Ordering::Relaxed) {
                crate::log::info(
                    "Android has no tray or taskbar: tray and taskbar-progress commands are ignored",
                );
            }
        }
    }
}
