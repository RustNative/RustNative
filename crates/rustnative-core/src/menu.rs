//! Native menu bars.
//!
//! A [`MenuBar`] is window chrome, not part of the declarative node tree
//! (`crate::node`): it is attached to a [`crate::window::Window`] once, at
//! window-creation time, and its selection events route through
//! `crate::event::Event::MenuAction` to the window's root component rather
//! than to a specific node owner — the same routing model window-lifecycle
//! events use (see `crate::window::event_window_id`).

use crate::identity::NodeId;

/// A single entry in a native [`MenuBar`]. Leaf items (those with no
/// children) dispatch `Event::MenuAction { item, .. }` when selected, using
/// the same stable [`NodeId::from_key`] identity already used to target UI
/// nodes. Items with children render as a native submenu instead of being
/// individually actionable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    id: NodeId,
    label: String,
    enabled: bool,
    checked: Option<bool>,
    separator: bool,
    children: Vec<MenuItem>,
    command: Option<crate::command::CommandId>,
}

impl MenuItem {
    /// A leaf menu item that dispatches `Event::MenuAction` with the id
    /// derived from `key` when selected.
    pub fn action(key: impl AsRef<str>, label: impl Into<String>) -> Self {
        Self {
            id: NodeId::from_key(key.as_ref()),
            label: label.into(),
            enabled: true,
            checked: None,
            separator: false,
            children: Vec::new(),
            command: None,
        }
    }

    /// A submenu item that groups other items instead of dispatching an
    /// action itself.
    pub fn submenu(
        key: impl AsRef<str>,
        label: impl Into<String>,
        children: impl IntoIterator<Item = MenuItem>,
    ) -> Self {
        Self {
            id: NodeId::from_key(key.as_ref()),
            label: label.into(),
            enabled: true,
            checked: None,
            separator: false,
            children: children.into_iter().collect(),
            command: None,
        }
    }

    /// A non-actionable visual divider between items in the same menu.
    #[must_use]
    pub fn separator() -> Self {
        Self {
            id: NodeId::from_key(""),
            label: String::new(),
            enabled: true,
            checked: None,
            separator: true,
            children: Vec::new(),
            command: None,
        }
    }

    /// Sets whether the item is enabled (selectable).
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Renders the item with a checkmark. Passing `false` renders an
    /// explicit (unchecked) checkable item rather than an ordinary one; use
    /// `action`/`submenu` alone to opt out of the checkable presentation.
    #[must_use]
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// An item bound to command `command`: selecting it invokes the
    /// command, and it is enabled, checked, and labelled with its shortcut
    /// from the command's current declaration (see [`crate::command`]).
    pub fn command(
        key: impl AsRef<str>,
        label: impl Into<String>,
        command: crate::command::CommandId,
    ) -> Self {
        let mut item = Self::action(key, label);
        item.command = Some(command);
        item
    }

    /// The command this item is bound to.
    #[must_use]
    pub const fn bound_command(&self) -> Option<crate::command::CommandId> {
        self.command
    }

    /// Finds the item with identity `id` in this item or its submenus.
    #[must_use]
    pub fn find(&self, id: NodeId) -> Option<&MenuItem> {
        if self.id == id && !self.separator {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    /// Returns the item's identity.
    #[must_use]
    pub fn id(&self) -> NodeId {
        self.id
    }
    /// Returns the item's display label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Returns whether the item is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    /// Returns the item's checked state, if it is a checkable item.
    #[must_use]
    pub fn is_checked(&self) -> Option<bool> {
        self.checked
    }
    /// Returns whether the item is a non-actionable visual separator.
    #[must_use]
    pub fn is_separator(&self) -> bool {
        self.separator
    }
    /// Returns whether the item is a submenu (has children).
    #[must_use]
    pub fn is_submenu(&self) -> bool {
        !self.children.is_empty()
    }
    /// Returns the item's children, if it is a submenu.
    #[must_use]
    pub fn children(&self) -> &[MenuItem] {
        &self.children
    }
}

/// A portable, declarative native menu bar attached to a `Window`.
///
/// A selection arrives as [`Event::MenuAction`] naming the item's own
/// identity. Menus are window chrome rather than part of the node tree, so
/// the event routes to the window's root component rather than to a node.
///
/// # Example
///
/// ```
/// use rustnative_core::{Event, MenuBar, MenuItem, NodeId, Size, Window, WindowId};
///
/// let menu = MenuBar::new([MenuItem::submenu(
///     "file",
///     "File",
///     [
///         MenuItem::action("file.open", "Open..."),
///         MenuItem::separator(),
///         MenuItem::action("file.quit", "Quit"),
///     ],
/// )]);
/// let window = Window::new("Editor", Size::new(800, 600)).with_menu(menu);
/// assert!(window.menu().is_some());
///
/// // A component matches on the item identity it declared:
/// let event = Event::MenuAction {
///     window: WindowId::PRIMARY,
///     item: NodeId::from_key("file.quit"),
/// };
/// if let Event::MenuAction { item, .. } = event {
///     assert_eq!(item, NodeId::from_key("file.quit"));
/// }
/// ```
///
/// [`Event::MenuAction`]: crate::Event::MenuAction
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuBar {
    items: Vec<MenuItem>,
}

impl MenuBar {
    /// Creates a menu bar from its top-level items.
    pub fn new(items: impl IntoIterator<Item = MenuItem>) -> Self {
        Self { items: items.into_iter().collect() }
    }

    /// Returns the menu bar's top-level items.
    #[must_use]
    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    /// Finds the item with identity `id` anywhere in the menu bar.
    #[must_use]
    pub fn find(&self, id: NodeId) -> Option<&MenuItem> {
        self.items.iter().find_map(|item| item.find(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_exposes_actions_submenus_and_separators() {
        let menu = MenuBar::new([
            MenuItem::submenu(
                "file",
                "File",
                [MenuItem::action("file.new", "New"), MenuItem::separator()],
            ),
            MenuItem::action("help", "Help").enabled(false),
        ]);
        assert_eq!(menu.items().len(), 2);
        assert!(menu.items()[0].is_submenu());
        assert_eq!(menu.items()[0].children().len(), 2);
        assert!(menu.items()[0].children()[1].is_separator());
        assert!(!menu.items()[1].is_enabled());
    }
}
