//! The command model (`C20`): an action with identity, bound everywhere.
//!
//! A command is declared once — its label, icon, shortcut, and whether it
//! is enabled or checked right now — by the component that can perform it
//! ([`crate::ComponentContext::command`]). Menus ([`crate::MenuItem::command`]),
//! buttons ([`crate::Node::with_command`]), keyboard shortcuts, and host
//! surfaces bind to it by [`CommandId`]. So a disabled command is disabled
//! everywhere at once, a menu shows the shortcut that actually works, and
//! invoking it from any of them delivers the same [`crate::Event::Command`]
//! to the same component.
//!
//! When several components declare the same command — a "Copy" in each of
//! two editors — the invocation is routed through the focus chain, as every
//! desktop host routes it: the component owning the focused node, then its
//! ancestors, then (if none of them declares it) the first declarer in
//! tree order.
//!
//! ```
//! use rustnative_core::{Command, CommandId, KeyCode, KeyModifiers, Shortcut};
//!
//! const SAVE: CommandId = CommandId::new("app.save");
//! let save = Command::new(SAVE, "Save")
//!     .shortcut(Shortcut::new(KeyCode::Character('s'), KeyModifiers { ctrl: true, ..KeyModifiers::default() }))
//!     .enabled(true);
//! assert_eq!(save.shortcut_label().as_deref(), Some("Ctrl+S"));
//! ```

use std::collections::HashMap;

use crate::event::{KeyCode, KeyModifiers};
use crate::identity::ComponentId;

/// A command's identity: a stable, namespaced name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommandId(&'static str);

impl CommandId {
    /// The command named `name` (for example `"app.save"`).
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// Its name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.0
    }
}

/// Well-known commands every host has a convention for. Declaring one of
/// these, rather than a private id, is what lets a backend bind it to the
/// host's own menu item or system shortcut.
pub mod standard {
    use super::CommandId;

    /// Undo the last change.
    pub const UNDO: CommandId = CommandId::new("rustnative.undo");
    /// Redo the last undone change.
    pub const REDO: CommandId = CommandId::new("rustnative.redo");
    /// Cut the selection.
    pub const CUT: CommandId = CommandId::new("rustnative.cut");
    /// Copy the selection.
    pub const COPY: CommandId = CommandId::new("rustnative.copy");
    /// Paste.
    pub const PASTE: CommandId = CommandId::new("rustnative.paste");
    /// Select everything.
    pub const SELECT_ALL: CommandId = CommandId::new("rustnative.select-all");
    /// Save the current document.
    pub const SAVE: CommandId = CommandId::new("rustnative.save");
    /// Open a document.
    pub const OPEN: CommandId = CommandId::new("rustnative.open");
    /// Create a new document.
    pub const NEW: CommandId = CommandId::new("rustnative.new");
    /// Close the current window or document.
    pub const CLOSE: CommandId = CommandId::new("rustnative.close");
    /// Find in the current view.
    pub const FIND: CommandId = CommandId::new("rustnative.find");
    /// Go back: pop a navigation stack, close a sheet, leave a mode.
    ///
    /// A component declares it enabled while it has somewhere to go back
    /// to (`NavigationStack::can_go_back`). A backend binds the host's own
    /// back to it — Android's system back and predictive back gesture, a
    /// mouse's back button — and asks before claiming the gesture
    /// (`Application::handles_back`): with no enabled declaration, the
    /// host's own back happens instead (leaving the application). Progress
    /// through a predictive gesture arrives first as
    /// [`crate::Event::BackProgress`].
    pub const BACK: CommandId = CommandId::new("rustnative.back");
}

/// A key combination that invokes a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    /// The key.
    pub key: KeyCode,
    /// The modifiers that must be held — exactly these.
    pub modifiers: KeyModifiers,
}

impl Shortcut {
    /// `key` with `modifiers`.
    #[must_use]
    pub const fn new(key: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }

    /// Ctrl (the host's primary modifier on Windows and Linux) plus `key`.
    #[must_use]
    pub const fn ctrl(key: KeyCode) -> Self {
        Self::new(key, KeyModifiers { shift: false, ctrl: true, alt: false, meta: false })
    }

    /// Whether a key press matches, comparing letters case-insensitively
    /// (Shift is compared as a modifier, not through the letter's case).
    #[must_use]
    pub fn matches(&self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        let same_key = match (self.key, key) {
            (KeyCode::Character(a), KeyCode::Character(b)) => a.eq_ignore_ascii_case(&b),
            (a, b) => a == b,
        };
        same_key && self.modifiers == modifiers
    }

    /// The label a Windows or Linux menu shows (`Ctrl+Shift+S`).
    #[must_use]
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        if self.modifiers.meta {
            parts.push("Win".to_owned());
        }
        parts.push(match self.key {
            KeyCode::Character(character) => character.to_uppercase().to_string(),
            KeyCode::Function(number) => format!("F{number}"),
            KeyCode::Enter => "Enter".to_owned(),
            KeyCode::Escape => "Esc".to_owned(),
            KeyCode::Delete => "Del".to_owned(),
            KeyCode::Tab => "Tab".to_owned(),
            KeyCode::Space => "Space".to_owned(),
            other => format!("{other:?}"),
        });
        parts.join("+")
    }
}

/// A command as one component declares it for the current render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    id: CommandId,
    label: String,
    icon: Option<String>,
    shortcut: Option<Shortcut>,
    enabled: bool,
    checked: Option<bool>,
}

impl Command {
    /// Command `id`, labelled `label`, enabled, with no shortcut.
    #[must_use]
    pub fn new(id: CommandId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), icon: None, shortcut: None, enabled: true, checked: None }
    }

    /// Sets its shortcut.
    #[must_use]
    pub const fn shortcut(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Sets whether it can be invoked right now.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Makes it a checkable command, currently `checked` or not.
    #[must_use]
    pub const fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Sets its icon, named as the host's icon set names it.
    #[must_use]
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Its identity.
    #[must_use]
    pub const fn id(&self) -> CommandId {
        self.id
    }

    /// Its label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Its icon name.
    #[must_use]
    pub fn icon_name(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    /// Its shortcut.
    #[must_use]
    pub const fn shortcut_key(&self) -> Option<Shortcut> {
        self.shortcut
    }

    /// Its shortcut, as a menu shows it.
    #[must_use]
    pub fn shortcut_label(&self) -> Option<String> {
        self.shortcut.map(|shortcut| shortcut.label())
    }

    /// Whether it can be invoked.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Whether it is checked, for a checkable command.
    #[must_use]
    pub const fn is_checked(&self) -> Option<bool> {
        self.checked
    }
}

/// Every command declared in a window's last render, with who declared it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandRegistry {
    declared: HashMap<CommandId, Vec<(ComponentId, Command)>>,
    order: Vec<CommandId>,
}

impl CommandRegistry {
    pub(crate) fn clear_owner(&mut self, owner: ComponentId) {
        for declarations in self.declared.values_mut() {
            declarations.retain(|(declarer, _)| *declarer != owner);
        }
        self.declared.retain(|_, declarations| !declarations.is_empty());
        let declared = &self.declared;
        self.order.retain(|id| declared.contains_key(id));
    }

    pub(crate) fn declare(&mut self, owner: ComponentId, command: Command) {
        let id = command.id;
        let declarations = self.declared.entry(id).or_default();
        declarations.retain(|(declarer, _)| *declarer != owner);
        declarations.push((owner, command));
        if !self.order.contains(&id) {
            self.order.push(id);
        }
    }

    /// The declaration of `id` that an invocation reaches, routed through
    /// `focus_chain` (the focused node's owner, then its ancestors).
    #[must_use]
    pub fn resolve(
        &self,
        id: CommandId,
        focus_chain: &[ComponentId],
    ) -> Option<(ComponentId, &Command)> {
        let declarations = self.declared.get(&id)?;
        focus_chain
            .iter()
            .find_map(|owner| declarations.iter().find(|(declarer, _)| declarer == owner))
            .or_else(|| declarations.first())
            .map(|(owner, command)| (*owner, command))
    }

    /// The command a shortcut invokes, if any is declared and enabled.
    #[must_use]
    pub fn for_shortcut(
        &self,
        key: KeyCode,
        modifiers: KeyModifiers,
        focus_chain: &[ComponentId],
    ) -> Option<(ComponentId, &Command)> {
        self.order.iter().find_map(|id| {
            let (owner, command) = self.resolve(*id, focus_chain)?;
            let matches = command.shortcut.is_some_and(|shortcut| shortcut.matches(key, modifiers));
            (matches && command.enabled).then_some((owner, command))
        })
    }

    /// The command `id` as the focus chain sees it — what a menu item or a
    /// button bound to it shows.
    #[must_use]
    pub fn state(&self, id: CommandId, focus_chain: &[ComponentId]) -> Option<&Command> {
        self.resolve(id, focus_chain).map(|(_, command)| command)
    }

    /// Every declared command, in first-declaration order — the palette a
    /// command-palette component lists (`C20-3`).
    pub fn commands(&self) -> impl Iterator<Item = &Command> {
        self.order
            .iter()
            .filter_map(|id| self.declared.get(id)?.first().map(|(_, command)| command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COPY: CommandId = CommandId::new("test.copy");

    #[test]
    fn a_command_is_routed_to_the_focused_declarer_first() {
        let mut registry = CommandRegistry::default();
        let (first, second) = (ComponentId::ROOT, ComponentId::next(&mut 7));
        registry.declare(first, Command::new(COPY, "Copy"));
        registry.declare(second, Command::new(COPY, "Copy selection"));
        assert_eq!(registry.resolve(COPY, &[second]).map(|(owner, _)| owner), Some(second));
        assert_eq!(registry.resolve(COPY, &[]).map(|(owner, _)| owner), Some(first));
        registry.clear_owner(second);
        assert_eq!(registry.resolve(COPY, &[second]).map(|(owner, _)| owner), Some(first));
    }

    #[test]
    fn a_disabled_command_does_not_answer_its_shortcut() {
        let mut registry = CommandRegistry::default();
        let shortcut = Shortcut::ctrl(KeyCode::Character('c'));
        registry.declare(
            ComponentId::ROOT,
            Command::new(COPY, "Copy").shortcut(shortcut).enabled(false),
        );
        let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
        assert!(registry.for_shortcut(KeyCode::Character('C'), ctrl, &[]).is_none());
        registry.declare(ComponentId::ROOT, Command::new(COPY, "Copy").shortcut(shortcut));
        assert!(registry.for_shortcut(KeyCode::Character('C'), ctrl, &[]).is_some());
        assert!(
            registry.for_shortcut(KeyCode::Character('c'), KeyModifiers::default(), &[]).is_none()
        );
    }

    #[test]
    fn shortcut_labels_follow_the_host_convention() {
        let shortcut = Shortcut::new(
            KeyCode::Character('s'),
            KeyModifiers { ctrl: true, shift: true, ..KeyModifiers::default() },
        );
        assert_eq!(shortcut.label(), "Ctrl+Shift+S");
        assert_eq!(Shortcut::new(KeyCode::Function(5), KeyModifiers::default()).label(), "F5");
    }
}
