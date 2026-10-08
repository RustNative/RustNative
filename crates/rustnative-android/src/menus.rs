//! A window's menu bar as its activity's options menu (the action bar's
//! overflow, and the keyboard shortcuts a hardware keyboard lists with
//! Meta+/): `RnMenus`.
//!
//! The menu is flattened into parallel arrays — each entry naming its
//! parent, so submenus survive — and rebuilt by the activity when it
//! changes. Items bound to a command follow it (enabled, checked, and its
//! shortcut), as on every other backend; choosing one invokes the command,
//! and choosing a plain item delivers `Event::MenuAction`.

use rustnative_core::{CommandId, MenuBar, MenuItem, NodeId};

/// One flattened menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: NodeId,
    pub(crate) label: String,
    pub(crate) parent: i32,
    /// Bit 0 enabled, bit 1 checkable, bit 2 checked, bit 3 submenu, bit 4
    /// separator (`RnMenus.Model.flags`).
    pub(crate) flags: i32,
    pub(crate) shortcut: Option<char>,
    pub(crate) command: Option<CommandId>,
}

/// A command-bound item's state: enabled, checked (when checkable), and
/// its shortcut's letter.
type CommandState = Option<(bool, Option<bool>, Option<char>)>;

/// The menu bar, flattened, with each command-bound item's state as
/// `state` answers it.
pub(crate) fn flatten(bar: &MenuBar, state: &dyn Fn(CommandId) -> CommandState) -> Vec<Entry> {
    let mut entries = Vec::new();
    for item in bar.items() {
        push(item, -1, state, &mut entries);
    }
    entries
}

fn push(
    item: &MenuItem,
    parent: i32,
    state: &dyn Fn(CommandId) -> CommandState,
    entries: &mut Vec<Entry>,
) {
    let command = item.bound_command();
    let (enabled, checked, shortcut) =
        command.and_then(state).unwrap_or((item.is_enabled(), item.is_checked(), None));
    let mut flags = 0;
    if enabled {
        flags |= 1;
    }
    if let Some(checked) = checked {
        flags |= 2;
        if checked {
            flags |= 4;
        }
    }
    if item.is_submenu() {
        flags |= 8;
    }
    if item.is_separator() {
        flags |= 16;
    }
    let index = i32::try_from(entries.len()).unwrap_or(i32::MAX);
    entries.push(Entry {
        id: item.id(),
        label: item.label().to_owned(),
        parent,
        flags,
        shortcut,
        command,
    });
    for child in item.children() {
        push(child, index, state, entries);
    }
}

#[cfg(target_os = "android")]
pub(crate) use platform::{chosen, sync};

#[cfg(target_os = "android")]
mod platform {
    use rustnative_core::{Event, KeyCode, WindowId};

    use crate::Error;
    use crate::jni_host::{Arg, Class, call_static};
    use crate::registry::WindowRegistry;

    /// Rebuilds each window's options menu where it changed.
    pub(crate) fn sync(registry: &mut WindowRegistry) -> Result<(), Error> {
        let windows: Vec<WindowId> = registry.windows.keys().copied().collect();
        for window in windows {
            let Some(bar) = registry.with_application(|application| {
                application.window_for(window).and_then(|definition| definition.menu().cloned())
            }) else {
                continue;
            };
            let focused = registry.windows.get(&window).and_then(|runtime| runtime.input.focused);
            let entries = registry.with_application(|application| {
                super::flatten(&bar, &|command| {
                    application.command_state(window, command, focused).map(|declared| {
                        let shortcut =
                            declared.shortcut_key().and_then(|shortcut| match shortcut.key {
                                KeyCode::Character(character) => Some(character),
                                _ => None,
                            });
                        (declared.is_enabled(), declared.is_checked(), shortcut)
                    })
                })
            });
            let Some(runtime) = registry.windows.get_mut(&window) else { continue };
            if runtime.menu.as_ref() == Some(&entries) {
                continue;
            }
            let Some(activity) = runtime.activity.clone() else { continue };
            let ids: Vec<String> = (0..entries.len()).map(|index| index.to_string()).collect();
            let labels: Vec<String> = entries.iter().map(|entry| entry.label.clone()).collect();
            let parents: Vec<i32> = entries.iter().map(|entry| entry.parent).collect();
            let flags: Vec<i32> = entries.iter().map(|entry| entry.flags).collect();
            let shortcuts: Vec<String> = entries
                .iter()
                .map(|entry| entry.shortcut.map(String::from).unwrap_or_default())
                .collect();
            call_static(
                Class::Menus,
                "set",
                "(Ldev/rustnative/android/RnActivity;[Ljava/lang/String;[Ljava/lang/String;[I[I[Ljava/lang/String;)V",
                &[
                    Arg::Obj(&activity),
                    Arg::Strs(&ids),
                    Arg::Strs(&labels),
                    Arg::Ints(&parents),
                    Arg::Ints(&flags),
                    Arg::Strs(&shortcuts),
                ],
            )?;
            runtime.menu = Some(entries);
        }
        Ok(())
    }

    /// The person chose menu entry `id` (its index) in `window`.
    pub(crate) fn chosen(
        registry: &mut WindowRegistry,
        window: WindowId,
        id: &str,
        _popup: bool,
    ) -> Result<(), Error> {
        let Ok(index) = id.parse::<usize>() else { return Ok(()) };
        let Some(entry) = registry
            .windows
            .get(&window)
            .and_then(|runtime| runtime.menu.as_ref())
            .and_then(|menu| menu.get(index))
            .cloned()
        else {
            return Ok(());
        };
        if let Some(command) = entry.command {
            let focused = registry.windows.get(&window).and_then(|runtime| runtime.input.focused);
            if registry.with_application(|application| {
                application.invoke_command(window, command, focused)
            }) {
                registry.render(window)?;
            }
            return registry.after_change(window);
        }
        registry.dispatch(window, Event::MenuAction { window, item: entry.id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_flattens_with_its_submenus_and_commands() {
        const SAVE: CommandId = CommandId::new("test.save");
        let bar = MenuBar::new([
            MenuItem::submenu(
                "file",
                "File",
                [
                    MenuItem::command("save", "Save", SAVE),
                    MenuItem::separator(),
                    MenuItem::action("quit", "Quit"),
                ],
            ),
            MenuItem::action("about", "About").enabled(false),
        ]);
        let entries =
            flatten(&bar, &|command| (command == SAVE).then_some((false, None, Some('s'))));
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].flags & 8, 8);
        assert_eq!(entries[1].parent, 0);
        assert_eq!(entries[1].flags & 1, 0, "the command is disabled, so the item is");
        assert_eq!(entries[1].shortcut, Some('s'));
        assert_eq!(entries[2].flags & 16, 16);
        assert_eq!(entries[4].parent, -1);
        assert_eq!(entries[4].flags & 1, 0);
    }
}
