//! Navigation stacks: screens pushed on top of each other, where going back
//! finds the screen below exactly as it was left.

use serde::{Deserialize, Serialize};

use crate::component::Callback;
use crate::layout::{ColumnStyle, EdgeInsets, LayoutStyle, SizeMode};
use crate::node::Node;

/// Identifies one entry of a [`NavigationStack`] for as long as it is on
/// the stack — and, since stacks are serializable, across runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntryId(u64);

impl EntryId {
    /// A component key unique to this entry, for composing the entry's
    /// screen as a keyed child: the same entry keeps the same child (and so
    /// the same state) however the stack above it changes.
    #[must_use]
    pub fn key(self) -> String {
        format!("nav-entry-{}", self.0)
    }
}

/// One screen on a [`NavigationStack`]: an application-defined route and
/// the identity it keeps while on the stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationEntry<R> {
    id: EntryId,
    route: R,
}

impl<R> NavigationEntry<R> {
    /// This entry's identity.
    #[must_use]
    pub const fn id(&self) -> EntryId {
        self.id
    }

    /// The route this entry shows.
    #[must_use]
    pub const fn route(&self) -> &R {
        &self.route
    }
}

/// A request to change a [`NavigationStack`], as sent by a [`Navigator`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationCommand<R> {
    /// Shows `route` on top of the current screen.
    Push(R),
    /// Returns to the screen below; does nothing on the root screen.
    Pop,
    /// Replaces the current screen with `route` (the one below is kept).
    Replace(R),
    /// Discards every screen and starts again from `route`.
    Reset(R),
}

/// A stack of screens, owned by the component that shows them.
///
/// The stack is plain data in that component's state; the screens it shows
/// are that component's keyed children, one per entry
/// ([`EntryId::key`]). Every entry is rendered on every render, and all
/// but the top one are [hidden](Node::hidden): so a screen pushed on top of
/// another neither unmounts nor rebuilds it, and popping reveals it with
/// its state, scroll position, and tasks intact. That is the whole design —
/// the managed component tree already keeps a keyed child alive for as long
/// as it keeps being rendered.
///
/// The root entry is never popped. The stack is `Serialize`/`Deserialize`,
/// so it can be restored across runs through
/// [`crate::ComponentContext::persisted`].
///
/// # Example
///
/// ```
/// use rustnative_core::{NavigationCommand, NavigationStack};
///
/// let mut stack = NavigationStack::new("home");
/// let home = stack.top().id();
/// stack.apply(NavigationCommand::Push("settings"));
/// assert_eq!(*stack.top().route(), "settings");
///
/// stack.apply(NavigationCommand::Pop);
/// assert_eq!(stack.top().id(), home, "the same entry, not a new one");
///
/// stack.apply(NavigationCommand::Pop);
/// assert_eq!(stack.len(), 1, "the root is never popped");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationStack<R> {
    entries: Vec<NavigationEntry<R>>,
    next: u64,
}

impl<R> NavigationStack<R> {
    /// A stack showing `root`.
    pub fn new(root: R) -> Self {
        Self { entries: vec![NavigationEntry { id: EntryId(0), route: root }], next: 1 }
    }

    fn entry(&mut self, route: R) -> NavigationEntry<R> {
        let id = EntryId(self.next);
        self.next = self.next.saturating_add(1);
        NavigationEntry { id, route }
    }

    /// Shows `route` on top, returning its entry's id.
    pub fn push(&mut self, route: R) -> EntryId {
        let entry = self.entry(route);
        let id = entry.id;
        self.entries.push(entry);
        id
    }

    /// Removes the top screen, returning its route; `None` on the root.
    pub fn pop(&mut self) -> Option<R> {
        if self.entries.len() <= 1 {
            return None;
        }
        self.entries.pop().map(|entry| entry.route)
    }

    /// Replaces the top screen with `route`, as a new entry.
    pub fn replace(&mut self, route: R) {
        let entry = self.entry(route);
        if let Some(top) = self.entries.last_mut() {
            *top = entry;
        }
    }

    /// Discards every screen and starts again from `route`.
    pub fn reset(&mut self, route: R) {
        let entry = self.entry(route);
        self.entries = vec![entry];
    }

    /// Applies a command, as delivered from a [`Navigator`].
    pub fn apply(&mut self, command: NavigationCommand<R>) {
        match command {
            NavigationCommand::Push(route) => {
                self.push(route);
            }
            NavigationCommand::Pop => {
                self.pop();
            }
            NavigationCommand::Replace(route) => self.replace(route),
            NavigationCommand::Reset(route) => self.reset(route),
        }
    }

    /// The screen being shown.
    #[must_use]
    pub fn top(&self) -> &NavigationEntry<R> {
        // A stack is never empty: every operation keeps at least one entry.
        &self.entries[self.entries.len() - 1]
    }

    /// Every entry, root first.
    #[must_use]
    pub fn entries(&self) -> &[NavigationEntry<R>] {
        &self.entries
    }

    /// How many screens are on the stack.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Always `false`: a stack has at least its root.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether there is a screen to go back to.
    #[must_use]
    pub fn can_go_back(&self) -> bool {
        self.entries.len() > 1
    }

    /// Renders every entry with `screen` into one container keyed `key`,
    /// hiding all but the top one.
    ///
    /// `screen` is where the owner composes each entry's screen, typically
    /// as `context.child_with_props(entry.id().key(), ...)`.
    pub fn view(
        &self,
        key: impl AsRef<str>,
        mut screen: impl FnMut(&NavigationEntry<R>) -> Node,
    ) -> Node {
        let top = self.top().id;
        let children = self
            .entries
            .iter()
            .map(|entry| screen(entry).hidden(entry.id != top))
            .collect::<Vec<_>>();
        Node::column_with_layout(
            key,
            children,
            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fill),
            ColumnStyle::new().padding(EdgeInsets::all(0)).gap(0),
        )
    }
}

/// The default size budget for a stack's saved state: 64 KiB, the order of
/// what hosts' restoration mechanisms accept (`C14`).
pub const SAVED_STATE_BUDGET: usize = 64 * 1024;

/// A stack's saved state did not fit its budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedStateTooLarge {
    /// How large it was, in bytes.
    pub size: usize,
    /// The budget.
    pub budget: usize,
    /// The entry whose route is largest, and its size — where to look.
    pub largest: Option<(EntryId, usize)>,
}

impl std::fmt::Display for SavedStateTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the navigation state is {} bytes, over its budget of {}",
            self.size, self.budget
        )?;
        if let Some((entry, size)) = self.largest {
            write!(f, "; the largest entry, {}, is {size} bytes", entry.key())?;
        }
        Ok(())
    }
}

impl std::error::Error for SavedStateTooLarge {}

impl<R: Serialize> NavigationStack<R> {
    /// The stack's saved state, for the host's restoration mechanism: each
    /// destination's route, which is the subset of its state it declares
    /// worth restoring (`C14`) — the rest is rebuilt. Refused when larger
    /// than `budget` bytes, naming the largest entry, so an oversized
    /// destination is found in development rather than dropped by the host.
    ///
    /// ```
    /// use rustnative_core::navigation::{NavigationStack, SAVED_STATE_BUDGET};
    ///
    /// let mut stack = NavigationStack::new("home".to_owned());
    /// stack.push("x".repeat(100));
    /// assert!(stack.saved_state(SAVED_STATE_BUDGET).is_ok());
    /// let error = stack.saved_state(64).unwrap_err();
    /// assert_eq!(error.largest.map(|(_, size)| size), Some(102));
    /// ```
    ///
    /// # Errors
    ///
    /// The encoded stack is larger than `budget`.
    pub fn saved_state(&self, budget: usize) -> Result<Vec<u8>, SavedStateTooLarge> {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        if bytes.len() <= budget {
            return Ok(bytes);
        }
        let largest = self
            .entries
            .iter()
            .map(|entry| {
                (entry.id, serde_json::to_vec(&entry.route).map_or(0, |route| route.len()))
            })
            .max_by_key(|(_, size)| *size);
        Err(SavedStateTooLarge { size: bytes.len(), budget, largest })
    }
}

/// A handle a screen uses to navigate the stack that shows it.
///
/// A navigator sends [`NavigationCommand`]s to the stack's owner through an
/// ordinary child-to-parent [`Callback`] — navigation is a message like any
/// other, applied by the owner in its `message` handler, and so it happens
/// after the event that asked for it has finished rather than in the middle
/// of it. `M` is the owner's message type, which only needs to be buildable
/// from a navigation command.
///
/// Cheap to clone, and comparable, so it can be passed to screens as props
/// without making them re-render.
#[derive(Debug)]
pub struct Navigator<R: 'static, M: 'static = NavigationCommand<R>> {
    callback: Callback<M>,
    _route: std::marker::PhantomData<fn(R)>,
}

impl<R: 'static, M: 'static> Clone for Navigator<R, M> {
    fn clone(&self) -> Self {
        Self { callback: self.callback.clone(), _route: std::marker::PhantomData }
    }
}

impl<R: 'static, M: 'static> PartialEq for Navigator<R, M> {
    fn eq(&self, other: &Self) -> bool {
        self.callback == other.callback
    }
}

impl<R: 'static, M: 'static> Eq for Navigator<R, M> {}

impl<R: 'static, M: From<NavigationCommand<R>> + 'static> Navigator<R, M> {
    /// A navigator that delivers to `callback` — the stack owner's, from
    /// `context.callback()`.
    #[must_use]
    pub fn new(callback: Callback<M>) -> Self {
        Self { callback, _route: std::marker::PhantomData }
    }

    /// Shows `route` on top of the current screen.
    pub fn push(&self, route: R) {
        self.callback.send(M::from(NavigationCommand::Push(route)));
    }

    /// Goes back one screen.
    pub fn pop(&self) {
        self.callback.send(M::from(NavigationCommand::Pop));
    }

    /// Replaces the current screen.
    pub fn replace(&self, route: R) {
        self.callback.send(M::from(NavigationCommand::Replace(route)));
    }

    /// Starts again from `route`.
    pub fn reset(&self, route: R) {
        self.callback.send(M::from(NavigationCommand::Reset(route)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_keep_their_identity_while_others_come_and_go() {
        let mut stack = NavigationStack::new("a");
        let root = stack.top().id();
        let b = stack.push("b");
        stack.push("c");
        stack.pop();
        assert_eq!(stack.top().id(), b, "b is the same entry it was before c was pushed");
        assert_eq!(stack.entries()[0].id(), root);
    }

    #[test]
    fn replace_and_reset_make_new_entries() {
        let mut stack = NavigationStack::new("a");
        let b = stack.push("b");
        stack.replace("c");
        assert_ne!(stack.top().id(), b, "a replaced screen is a new screen");
        assert_eq!(stack.len(), 2);
        stack.reset("home");
        assert_eq!(stack.len(), 1);
        assert_eq!(*stack.top().route(), "home");
    }

    #[test]
    fn ids_are_never_reused_even_after_popping() {
        let mut stack = NavigationStack::new(0);
        let first = stack.push(1);
        stack.pop();
        let second = stack.push(1);
        assert_ne!(first, second, "a new screen must not inherit a dead screen's state");
    }

    #[test]
    fn the_view_hides_every_screen_but_the_top_one() {
        let mut stack = NavigationStack::new("a");
        stack.push("b");
        let view = stack.view("nav", |entry| Node::label(entry.id().key(), *entry.route()));
        let Node::Column(column) = view else { panic!("a column") };
        let hidden = column.children().iter().map(Node::is_hidden).collect::<Vec<_>>();
        assert_eq!(hidden, vec![true, false]);
    }

    #[test]
    fn a_stack_round_trips_through_serialization() {
        let mut stack = NavigationStack::new("home".to_owned());
        stack.push("settings".to_owned());
        let json = serde_json::to_string(&stack).unwrap();
        let restored: NavigationStack<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, stack);
        let mut restored = restored;
        assert_ne!(restored.push("more".to_owned()), stack.top().id(), "ids continue, not restart");
    }
}
