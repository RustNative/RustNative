//! Menu bars (`rustnative_core::MenuBar`) as GTK menus.
//!
//! A window's menu bar is a `GMenu` model shown by a `GtkPopoverMenuBar`
//! above the window's content — the menu bar every GTK 4 application
//! shows. Each selectable item is an action in the window's `rn` action
//! group: activating it (by pointer, keyboard, or assistive technology)
//! delivers `Event::MenuAction`, and a checkable item's action carries its
//! check state, so GTK draws it as a check item.
//!
//! A command-bound item follows its command's live declaration after every
//! change: its action is enabled or not, its check state follows, and its
//! shortcut is shown through the item's `accel` attribute — which GTK only
//! displays; the shortcut itself is handled with the window's other command
//! shortcuts (`gtk::input`).
//!
//! The menu bar is in the window: GTK 4 exports only an application-wide
//! menu to a global menu bar, and a Rust Native menu belongs to a window,
//! so `Capability::GlobalMenuBar` is not advertised.

use gtk::prelude::*;
use gtk::{gio, glib};
use rustnative_core::command::{CommandId, Shortcut};
use rustnative_core::{Event, KeyCode, MenuBar, MenuItem, NodeId, WindowId};

use super::backend::{Work, post};

/// The action group's prefix.
const GROUP: &str = "rn";

/// One selectable item.
struct Entry {
    #[cfg_attr(not(test), allow(dead_code, reason = "how tests find an item's action"))]
    item: NodeId,
    label: String,
    action: gio::SimpleAction,
    command: Option<CommandId>,
    /// The menu the item is in, and its position there, so a changed
    /// shortcut can replace it.
    parent: gio::Menu,
    position: i32,
    /// The accelerator it shows.
    accel: Option<String>,
}

/// A window's realized menu bar.
pub(crate) struct WindowMenu {
    bar: gtk::PopoverMenuBar,
    entries: Vec<Entry>,
}

impl std::fmt::Debug for WindowMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowMenu").field("items", &self.entries.len()).finish_non_exhaustive()
    }
}

/// `shortcut` as a GTK accelerator (`<Control>s`).
pub(crate) fn accelerator(shortcut: Shortcut) -> Option<String> {
    let key = match shortcut.key {
        KeyCode::Enter => "Return".to_owned(),
        KeyCode::Space => "space".to_owned(),
        KeyCode::Tab => "Tab".to_owned(),
        KeyCode::Escape => "Escape".to_owned(),
        KeyCode::Backspace => "BackSpace".to_owned(),
        KeyCode::ArrowLeft => "Left".to_owned(),
        KeyCode::ArrowRight => "Right".to_owned(),
        KeyCode::ArrowUp => "Up".to_owned(),
        KeyCode::ArrowDown => "Down".to_owned(),
        KeyCode::Delete => "Delete".to_owned(),
        KeyCode::Insert => "Insert".to_owned(),
        KeyCode::Home => "Home".to_owned(),
        KeyCode::End => "End".to_owned(),
        KeyCode::PageUp => "Page_Up".to_owned(),
        KeyCode::PageDown => "Page_Down".to_owned(),
        KeyCode::Function(number) => format!("F{number}"),
        KeyCode::Character(character) => {
            use glib::translate::FromGlib as _;
            let lower = character.to_lowercase().next().unwrap_or(character);
            let keyval = gtk::gdk::unicode_to_keyval(u32::from(lower));
            // SAFETY: a keyval is a plain number; any value is a valid `Key`
            // (an unknown one has no name, answered below).
            unsafe { gtk::gdk::Key::from_glib(keyval) }.name()?.to_string()
        }
        _ => return None,
    };
    let modifiers = shortcut.modifiers;
    let mut accel = String::new();
    for (held, name) in [
        (modifiers.ctrl, "<Control>"),
        (modifiers.shift, "<Shift>"),
        (modifiers.alt, "<Alt>"),
        (modifiers.meta, "<Super>"),
    ] {
        if held {
            accel.push_str(name);
        }
    }
    accel.push_str(&key);
    gtk::accelerator_parse(&accel).is_some().then_some(accel)
}

fn menu_item(label: &str, action: &str, accel: Option<&str>) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), Some(&format!("{GROUP}.{action}")));
    if let Some(accel) = accel {
        item.set_attribute_value("accel", Some(&accel.to_variant()));
    }
    item
}

/// Builds `items` into `menu`, adding each selectable item's action to
/// `group`.
fn fill(
    menu: &gio::Menu,
    items: &[MenuItem],
    window: WindowId,
    group: &gio::SimpleActionGroup,
    entries: &mut Vec<Entry>,
) {
    // A separator starts a new section: GTK draws a line between sections.
    let mut section = gio::Menu::new();
    let flush = |section: &mut gio::Menu| {
        if section.n_items() > 0 {
            menu.append_section(None, section);
            *section = gio::Menu::new();
        }
    };
    for item in items {
        if item.is_separator() {
            flush(&mut section);
            continue;
        }
        if item.is_submenu() {
            let submenu = gio::Menu::new();
            fill(&submenu, item.children(), window, group, entries);
            section.append_submenu(Some(item.label()), &submenu);
            continue;
        }
        let name = format!("item{}", entries.len());
        let action = match item.is_checked() {
            Some(checked) => gio::SimpleAction::new_stateful(&name, None, &checked.to_variant()),
            None => gio::SimpleAction::new(&name, None),
        };
        action.set_enabled(item.is_enabled());
        let id = item.id();
        action.connect_activate(move |_, _| {
            post(Work::Event(window, Event::MenuAction { window, item: id }));
        });
        // A checkable item's state changes only as the application says.
        action.connect_change_state(|_, _| {});
        group.add_action(&action);
        let position = section.n_items();
        section.append_item(&menu_item(item.label(), &name, None));
        entries.push(Entry {
            item: id,
            label: item.label().to_owned(),
            action,
            command: item.bound_command(),
            parent: section.clone(),
            position,
            accel: None,
        });
    }
    flush(&mut section);
}

impl WindowMenu {
    /// Builds `bar` for `window`, its actions installed on `host`.
    pub(crate) fn build(bar: &MenuBar, window: WindowId, host: &impl IsA<gtk::Widget>) -> Self {
        let model = gio::Menu::new();
        let group = gio::SimpleActionGroup::new();
        let mut entries = Vec::new();
        // Top-level items are the bar's own entries, not a section.
        for item in bar.items() {
            if item.is_submenu() {
                let submenu = gio::Menu::new();
                fill(&submenu, item.children(), window, &group, &mut entries);
                model.append_submenu(Some(item.label()), &submenu);
            } else if !item.is_separator() {
                // A menu bar shows only menus: an action at the top level
                // opens a menu holding just itself.
                let submenu = gio::Menu::new();
                fill(&submenu, std::slice::from_ref(item), window, &group, &mut entries);
                model.append_submenu(Some(item.label()), &submenu);
            }
        }
        host.insert_action_group(GROUP, Some(&group));
        Self { bar: gtk::PopoverMenuBar::from_model(Some(&model)), entries }
    }

    /// The menu bar widget.
    pub(crate) fn widget(&self) -> &gtk::PopoverMenuBar {
        &self.bar
    }

    /// Brings every command-bound item up to its command's declaration:
    /// `state(command)` is the command's enabled, checked, and shortcut
    /// (`None`: not declared, so disabled).
    pub(crate) fn refresh(
        &mut self,
        state: impl Fn(CommandId) -> Option<(bool, Option<bool>, Option<Shortcut>)>,
    ) {
        for entry in &mut self.entries {
            let Some(command) = entry.command else { continue };
            let (enabled, checked, shortcut) = state(command).unwrap_or((false, None, None));
            if entry.action.is_enabled() != enabled {
                entry.action.set_enabled(enabled);
            }
            if let Some(checked) = checked {
                if entry.action.state().and_then(|state| state.get::<bool>()) != Some(checked) {
                    entry.action.set_state(&checked.to_variant());
                }
            }
            let accel = shortcut.and_then(accelerator);
            if accel != entry.accel {
                // A model item cannot change; the shortcut's label is a new
                // item in the same place.
                let name = entry.action.name();
                entry.parent.remove(entry.position);
                entry
                    .parent
                    .insert_item(entry.position, &menu_item(&entry.label, &name, accel.as_deref()));
                entry.accel = accel;
            }
        }
    }

    /// The action for `item`, for tests.
    #[cfg(test)]
    pub(crate) fn action_of(&self, item: NodeId) -> Option<gio::SimpleAction> {
        self.entries.iter().find(|entry| entry.item == item).map(|entry| entry.action.clone())
    }

    /// The accelerator `item` shows, for tests.
    #[cfg(test)]
    pub(crate) fn accel_of(&self, item: NodeId) -> Option<String> {
        let entry = self.entries.iter().find(|entry| entry.item == item)?;
        entry
            .parent
            .item_attribute_value(entry.position, "accel", Some(glib::VariantTy::STRING))
            .and_then(|value| value.get::<String>())
    }
}

#[cfg(test)]
mod tests {
    use rustnative_core::KeyModifiers;

    use super::*;

    #[test]
    fn shortcuts_become_gtk_accelerators() {
        super::super::testing::on_gtk(|| {
            let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
            let shift_ctrl = KeyModifiers { ctrl: true, shift: true, ..KeyModifiers::default() };
            assert_eq!(
                accelerator(Shortcut { key: KeyCode::Character('S'), modifiers: ctrl }).as_deref(),
                Some("<Control>s")
            );
            assert_eq!(
                accelerator(Shortcut {
                    key: KeyCode::Function(5),
                    modifiers: KeyModifiers::default()
                })
                .as_deref(),
                Some("F5")
            );
            assert_eq!(
                accelerator(Shortcut { key: KeyCode::Character('Z'), modifiers: shift_ctrl })
                    .as_deref(),
                Some("<Control><Shift>z")
            );
            assert_eq!(accelerator(Shortcut { key: KeyCode::Unknown(0), modifiers: ctrl }), None);
        });
    }
}
