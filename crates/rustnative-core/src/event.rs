//! Semantic input events delivered from a platform backend to a component.

use crate::accessibility::AccessibleAction;
pub use crate::accessibility::{AccessibilityInfo, AccessibilityRole};
use crate::animation::AnimatedProperty;
use crate::graphics::SurfaceId;
use crate::identity::{NodeId, WindowId};
use crate::input::{
    ClipboardAction, Composition, DragData, GamepadInput, Gesture, PointerEvent, Scalar, WheelDelta,
};
use crate::layout::{Point, Size};
use crate::lifecycle::Lifecycle;
use crate::virtualization::VirtualRange;
use crate::window::WindowPresentation;

/// The name of every [`Event`] variant, in declaration order — what
/// `rustnative describe` lists (its tests check this against the enum).
pub const EVENT_NAMES: &[&str] = &[
    "Click",
    "FocusGained",
    "FocusLost",
    "KeyDown",
    "TextInput",
    "TextChanged",
    "WindowResized",
    "WindowMoved",
    "WindowCloseRequested",
    "WindowStateChanged",
    "MenuAction",
    "KeyUp",
    "PointerDown",
    "PointerMove",
    "PointerUp",
    "PointerCancel",
    "PointerEnter",
    "PointerLeave",
    "Wheel",
    "Gesture",
    "Composition",
    "Clipboard",
    "ClipboardChanged",
    "SurfaceAction",
    "DragEnter",
    "DragOver",
    "DragLeave",
    "Drop",
    "AccessibilityAction",
    "TabSelected",
    "Toggled",
    "ValueChanged",
    "SelectionChanged",
    "DateChanged",
    "Command",
    "BackProgress",
    "ShareReceived",
    "DeepLink",
    "Lifecycle",
    "SurfaceResized",
    "VisibleRangeChanged",
    "AnimationFinished",
    "Gamepad",
];

/// Input produced by a platform backend and delivered to the active
/// component tree.
///
/// `#[non_exhaustive]`: a future platform capability (drag-and-drop, IME
/// composition, pointer capture, ...) can add a variant without breaking
/// every downstream `match` — see `PLAN.md`'s advanced-input milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A pointer press-and-release (or equivalent activation, e.g. Space/
    /// Enter on a focused control) completed on `target`.
    Click {
        /// The node that was activated.
        target: NodeId,
    },
    /// `target` became the focused node.
    FocusGained {
        /// The node that gained focus.
        target: NodeId,
    },
    /// `target` was the focused node and is no longer.
    FocusLost {
        /// The node that lost focus.
        target: NodeId,
    },
    /// A key was pressed while `target` (or, if `None`, no specific node)
    /// had focus.
    KeyDown {
        /// The focused node the key was delivered to, or `None` if no node
        /// currently has focus.
        target: Option<NodeId>,
        /// The key that was pressed.
        key: KeyCode,
        /// Which modifier keys were held down at the same time.
        modifiers: KeyModifiers,
    },
    /// Committed text input (e.g. from an IME or a printable keystroke),
    /// delivered independently of [`Event::KeyDown`] since one physical key
    /// press can produce zero, one, or many characters.
    TextInput {
        /// The focused node the text was delivered to, or `None` if no node
        /// currently has focus.
        target: Option<NodeId>,
        /// The committed text.
        text: String,
    },
    /// A text-input control's content changed to `value`.
    TextChanged {
        /// The text-input node whose content changed.
        target: NodeId,
        /// The control's new, complete text content.
        value: String,
    },
    /// A window was resized to `size`.
    WindowResized {
        /// The window that was resized.
        window: WindowId,
        /// The window's new client-area size.
        size: Size,
    },
    /// A window was moved to `position`.
    WindowMoved {
        /// The window that was moved.
        window: WindowId,
        /// The window's new top-left position, in screen coordinates.
        position: Point,
    },
    /// The person asked to close a window (e.g. clicked its close button);
    /// the window remains open until the application actually removes it.
    WindowCloseRequested {
        /// The window the close request applies to.
        window: WindowId,
    },
    /// A window's presentation (minimized/maximized/restored) changed.
    WindowStateChanged {
        /// The window whose presentation changed.
        window: WindowId,
        /// The window's new presentation.
        state: WindowPresentation,
    },
    /// A native menu item was selected. Menus are window chrome rather than
    /// part of the declarative node tree, so — like window-lifecycle events
    /// — this always routes to the window's root component rather than to a
    /// specific node owner.
    MenuAction {
        /// The window whose menu bar the selected item belongs to.
        window: WindowId,
        /// The identity of the menu item that was selected.
        item: NodeId,
    },
    /// A key was released. Delivered like [`Event::KeyDown`].
    KeyUp {
        /// The focused node, or `None` if no node has focus.
        target: Option<NodeId>,
        /// The key that was released.
        key: KeyCode,
        /// Which modifier keys were held down at the same time.
        modifiers: KeyModifiers,
    },
    /// A pointer contact began (button pressed, finger or pen touched) on
    /// a node with pointer interest (see [`crate::InputInterest`]).
    PointerDown {
        /// The interested node.
        target: NodeId,
        /// The sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// A pointer moved over (or, while captured, anywhere on behalf of) a
    /// node with pointer interest.
    PointerMove {
        /// The interested node.
        target: NodeId,
        /// The sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// A pointer contact ended.
    PointerUp {
        /// The interested node.
        target: NodeId,
        /// The sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// A pointer contact was taken away without ending normally — pointer
    /// capture was lost, or the window was deactivated mid-press. Treat
    /// like an up event that must not commit anything.
    PointerCancel {
        /// The interested node.
        target: NodeId,
        /// The last sample, in `target`'s local coordinates.
        pointer: PointerEvent,
    },
    /// The pointer entered a node with pointer interest (hover began).
    PointerEnter {
        /// The interested node.
        target: NodeId,
    },
    /// The pointer left a node with pointer interest (hover ended).
    PointerLeave {
        /// The interested node.
        target: NodeId,
    },
    /// A scroll wheel or trackpad scrolled over a node with wheel interest.
    Wheel {
        /// The interested node.
        target: NodeId,
        /// How far, and in which unit.
        delta: WheelDelta,
    },
    /// A gesture was recognized on a node with gesture interest.
    Gesture {
        /// The interested node.
        target: NodeId,
        /// The gesture, in `target`'s local coordinates.
        gesture: Gesture,
    },
    /// An input-method composition step for the focused node.
    Composition {
        /// The focused node, or `None` if no node has focus.
        target: Option<NodeId>,
        /// The composition step.
        composition: Composition,
    },
    /// The person performed a clipboard operation on the focused node.
    Clipboard {
        /// The focused node, or `None` if no node has focus.
        target: Option<NodeId>,
        /// The operation.
        action: ClipboardAction,
    },
    /// The system clipboard's content changed (from any application).
    /// Routed like a window-lifecycle event, to the window's root
    /// component.
    ClipboardChanged {
        /// The window being notified.
        window: WindowId,
    },
    /// A surface beyond the window was used: a tray menu item chosen, the
    /// tray icon clicked (`"activate"`), a notification clicked
    /// (`"notification"`). Routed like a menu choice, to the window's root
    /// component (`PLAN.md` Milestone 57, [`crate::surfaces`]).
    SurfaceAction {
        /// The window whose surface it was (the primary window).
        window: WindowId,
        /// Which surface.
        surface: crate::capability::SurfaceKind,
        /// The action: a menu item's id, or one of the constants in
        /// [`crate::surfaces`].
        action: String,
    },
    /// A drag entered a drop-target node. Answer with
    /// `ComponentContext::input().set_drop_effect(...)` to accept it.
    DragEnter {
        /// The drop-target node.
        target: NodeId,
        /// What is being dragged.
        data: DragData,
        /// Pointer position in `target`'s local coordinates.
        position: Point,
    },
    /// A drag moved over a drop-target node.
    DragOver {
        /// The drop-target node.
        target: NodeId,
        /// What is being dragged.
        data: DragData,
        /// Pointer position in `target`'s local coordinates.
        position: Point,
    },
    /// A drag left a drop-target node without dropping.
    DragLeave {
        /// The drop-target node.
        target: NodeId,
    },
    /// Data was dropped on a drop-target node that accepted it.
    Drop {
        /// The drop-target node.
        target: NodeId,
        /// What was dropped.
        data: DragData,
        /// Drop position in `target`'s local coordinates.
        position: Point,
    },
    /// Assistive technology asked `target` (or one of its virtual
    /// elements) to perform an operation it declared support for — see
    /// [`crate::AccessibleAction`] for why this is not a synthesized click.
    AccessibilityAction {
        /// The node whose semantics were acted on.
        target: NodeId,
        /// The virtual element acted on, if it was one of the node's
        /// [`crate::VirtualElement`]s rather than the node itself.
        element: Option<NodeId>,
        /// What was asked.
        action: AccessibleAction,
    },
    /// The person chose tab `index` of a tab bar.
    ///
    /// The tab bar does not change on its own: the component renders the
    /// new selection, exactly as a text field's value follows
    /// `TextChanged`.
    TabSelected {
        /// The tab bar.
        target: NodeId,
        /// The chosen tab.
        index: usize,
    },
    /// A check box, switch, or radio button was turned on or off by the
    /// person (see [`crate::control`]). The component answers by rendering
    /// the new state.
    Toggled {
        /// The control.
        target: NodeId,
        /// Its new state.
        on: bool,
    },
    /// A slider or spinner was moved to `value`.
    ValueChanged {
        /// The control.
        target: NodeId,
        /// The new value.
        value: i64,
    },
    /// A select or list box's choice changed.
    SelectionChanged {
        /// The control.
        target: NodeId,
        /// The chosen item, if any.
        index: Option<usize>,
    },
    /// A date picker's date changed.
    DateChanged {
        /// The control.
        target: NodeId,
        /// The new date.
        date: crate::control::CalendarDate,
    },
    /// A command this component declared was invoked — from a menu, a
    /// button bound to it, its shortcut, or a host surface (see
    /// [`crate::command`]). Delivered to the declaring component the focus
    /// chain reaches.
    Command {
        /// The command.
        id: crate::command::CommandId,
    },
    /// The person is partway through the host's back gesture (Android's
    /// predictive back), which will invoke
    /// [`crate::command::standard::BACK`] if they finish it. Delivered to
    /// the component that declared `BACK` enabled, so a screen can preview
    /// what going back reveals; a finished gesture arrives as
    /// [`Event::Command`], an abandoned one as [`BackPhase::Cancelled`].
    BackProgress {
        /// The window the gesture is in.
        window: crate::identity::WindowId,
        /// How far along it is.
        phase: BackPhase,
    },
    /// Content another application shared with this one through the host's
    /// share sheet (an Android share target, an iOS share extension).
    /// Delivered to the primary window's root component.
    ShareReceived {
        /// What was shared.
        share: SharedContent,
    },
    /// The application was asked to open `url` — launched with it, or
    /// handed it by a second launch while already running.
    ///
    /// Delivered to the primary window's root component; route it with
    /// [`crate::Router`] (see [`crate::url_path`]).
    DeepLink {
        /// The URL, exactly as received.
        url: String,
    },
    /// The application is about to be suspended, resumed, or terminated.
    ///
    /// Persisted state is flushed to its store *before* this is delivered
    /// for `Suspending` and `Terminating`, so a component never has to save
    /// anything itself; it is told so it can do what is specific to it
    /// (pause playback, close a connection).
    Lifecycle(Lifecycle),
    /// A native surface was realized, or changed size.
    ///
    /// Delivered after layout gives the surface its first size and after
    /// every change to it, so an application attaching a swapchain creates
    /// it here and resizes it here. `surface` is what the backend's handle
    /// lookup takes (on Windows, `rustnative_windows::native_surface`).
    SurfaceResized {
        /// The surface node.
        target: NodeId,
        /// The backend's identifier for the surface.
        surface: SurfaceId,
        /// The surface's size in physical pixels — what a swapchain is
        /// created with.
        size: Size,
        /// Physical pixels per layout unit (1.0 at 96 DPI on Windows).
        scale_factor: Scalar,
    },
    /// A virtual list needs a different window of items realized.
    ///
    /// Raised when scrolling (or a resize, or newly measured item extents)
    /// moves the visible range, and **only** then: scrolling within a range
    /// is a native viewport transform no component hears about. A component
    /// answers this by rendering the items in `range`, each tagged with
    /// [`Node::with_item_index`](crate::Node::with_item_index).
    VisibleRangeChanged {
        /// The virtual list whose visible range changed.
        target: NodeId,
        /// The items it should now realize.
        range: VirtualRange,
    },
    /// An animation on `target` ended — because it ran out, not because
    /// it was cancelled. Delivered to the component that owns the node, so
    /// one animation can lead to the next.
    AnimationFinished {
        /// The node that was animating.
        target: NodeId,
        /// Which property finished.
        property: AnimatedProperty,
    },
    /// A game controller changed, delivered to the first node (in
    /// declarative order) of the active window that declared gamepad
    /// interest.
    Gamepad {
        /// The interested node.
        target: NodeId,
        /// Which controller (slot number, stable while it stays connected).
        gamepad: u32,
        /// What changed.
        input: GamepadInput,
    },
}

impl Event {
    /// Returns the node this event targets, if any. Window-lifecycle and
    /// menu events have no node target: see `crate::window::event_window_id`
    /// for how those are routed instead.
    #[must_use]
    pub const fn target(&self) -> Option<NodeId> {
        match self {
            Self::Click { target }
            | Self::FocusGained { target }
            | Self::FocusLost { target }
            | Self::TextChanged { target, .. }
            | Self::PointerDown { target, .. }
            | Self::PointerMove { target, .. }
            | Self::PointerUp { target, .. }
            | Self::PointerCancel { target, .. }
            | Self::PointerEnter { target }
            | Self::PointerLeave { target }
            | Self::Wheel { target, .. }
            | Self::Gesture { target, .. }
            | Self::DragEnter { target, .. }
            | Self::DragOver { target, .. }
            | Self::DragLeave { target }
            | Self::Drop { target, .. }
            | Self::Gamepad { target, .. }
            | Self::AccessibilityAction { target, .. }
            | Self::AnimationFinished { target, .. }
            | Self::VisibleRangeChanged { target, .. }
            | Self::SurfaceResized { target, .. }
            | Self::TabSelected { target, .. }
            | Self::Toggled { target, .. }
            | Self::ValueChanged { target, .. }
            | Self::SelectionChanged { target, .. }
            | Self::DateChanged { target, .. } => Some(*target),
            Self::KeyDown { target, .. }
            | Self::KeyUp { target, .. }
            | Self::TextInput { target, .. }
            | Self::Composition { target, .. }
            | Self::Clipboard { target, .. } => *target,
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. }
            | Self::SurfaceAction { .. }
            | Self::ClipboardChanged { .. }
            | Self::DeepLink { .. }
            | Self::Command { .. }
            | Self::BackProgress { .. }
            | Self::ShareReceived { .. }
            | Self::Lifecycle(_) => None,
        }
    }

    /// Rewrites a platform-facing target to the component-local key exposed
    /// to `Component::update`. Window and menu events have no node target.
    pub(crate) fn with_local_target(mut self, target: Option<NodeId>) -> Self {
        match &mut self {
            Self::Click { target: current }
            | Self::FocusGained { target: current }
            | Self::FocusLost { target: current }
            | Self::TextChanged { target: current, .. }
            | Self::PointerDown { target: current, .. }
            | Self::PointerMove { target: current, .. }
            | Self::PointerUp { target: current, .. }
            | Self::PointerCancel { target: current, .. }
            | Self::PointerEnter { target: current }
            | Self::PointerLeave { target: current }
            | Self::Wheel { target: current, .. }
            | Self::Gesture { target: current, .. }
            | Self::DragEnter { target: current, .. }
            | Self::DragOver { target: current, .. }
            | Self::DragLeave { target: current }
            | Self::Drop { target: current, .. }
            | Self::Gamepad { target: current, .. }
            | Self::AccessibilityAction { target: current, .. }
            | Self::AnimationFinished { target: current, .. }
            | Self::VisibleRangeChanged { target: current, .. }
            | Self::SurfaceResized { target: current, .. }
            | Self::TabSelected { target: current, .. }
            | Self::Toggled { target: current, .. }
            | Self::ValueChanged { target: current, .. }
            | Self::SelectionChanged { target: current, .. }
            | Self::DateChanged { target: current, .. } => {
                if let Some(target) = target {
                    *current = target;
                }
            }
            Self::KeyDown { target: current, .. }
            | Self::KeyUp { target: current, .. }
            | Self::TextInput { target: current, .. }
            | Self::Composition { target: current, .. }
            | Self::Clipboard { target: current, .. } => {
                *current = target;
            }
            Self::WindowResized { .. }
            | Self::WindowMoved { .. }
            | Self::WindowCloseRequested { .. }
            | Self::WindowStateChanged { .. }
            | Self::MenuAction { .. }
            | Self::SurfaceAction { .. }
            | Self::ClipboardChanged { .. }
            | Self::DeepLink { .. }
            | Self::Command { .. }
            | Self::BackProgress { .. }
            | Self::ShareReceived { .. }
            | Self::Lifecycle(_) => {}
        }
        self
    }
}

/// How far the host's back gesture has gone ([`Event::BackProgress`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BackPhase {
    /// The gesture began, from `edge`.
    Started {
        /// The screen edge the swipe came from.
        edge: BackEdge,
    },
    /// The gesture moved: `progress` runs from 0 (just started) to 1 (as
    /// far as it goes), following the person's finger.
    Progressed {
        /// How far along, 0 to 1.
        progress: crate::input::Scalar,
    },
    /// The person let go without finishing; nothing goes back.
    Cancelled,
}

/// The screen edge a back gesture came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackEdge {
    /// The left edge.
    Left,
    /// The right edge.
    Right,
    /// No edge: a button or key started it.
    None,
}

/// What another application shared ([`Event::ShareReceived`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SharedContent {
    /// Shared text, if any.
    pub text: Option<String>,
    /// A subject or title that came with it.
    pub subject: Option<String>,
    /// Shared items (files, images), each a URI this application was
    /// granted read access to, with its MIME type.
    pub items: Vec<SharedItem>,
}

/// One shared item ([`SharedContent::items`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedItem {
    /// Where to read it (a `content://` URI on Android).
    pub uri: String,
    /// Its MIME type, as the sender declared it.
    pub mime_type: String,
}

/// A platform-independent keyboard key. Backends translate their native
/// virtual-key constants into this set rather than exposing them directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum KeyCode {
    /// The Enter/Return key.
    Enter,
    /// The Space bar.
    Space,
    /// The Tab key.
    Tab,
    /// The Escape key.
    Escape,
    /// The Backspace key.
    Backspace,
    /// The left arrow key.
    ArrowLeft,
    /// The right arrow key.
    ArrowRight,
    /// The up arrow key.
    ArrowUp,
    /// The down arrow key.
    ArrowDown,
    /// The Delete (forward delete) key.
    Delete,
    /// The Insert key.
    Insert,
    /// The Home key.
    Home,
    /// The End key.
    End,
    /// The Page Up key.
    PageUp,
    /// The Page Down key.
    PageDown,
    /// A function key, `F1` through `F24`, carrying its number.
    Function(u8),
    /// A printable character key, carrying the character it produces.
    Character(char),
    /// A key this crate does not yet name explicitly, carrying the
    /// backend's native virtual-key code for diagnostics/escape-hatch use.
    Unknown(u32),
}

/// Which modifier keys were held down when a [`KeyCode`] was produced.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent physical keys, each held or not; not a state machine in disguise"
)]
pub struct KeyModifiers {
    /// Whether either Shift key was held down.
    pub shift: bool,
    /// Whether either Ctrl key was held down.
    pub ctrl: bool,
    /// Whether either Alt key was held down.
    pub alt: bool,
    /// Whether the platform's "meta" key (Windows key, Command, Super) was
    /// held down.
    pub meta: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_target_is_none_for_window_and_menu_events() {
        assert_eq!(Event::WindowCloseRequested { window: WindowId::PRIMARY }.target(), None);
        assert_eq!(
            Event::MenuAction { window: WindowId::PRIMARY, item: NodeId::from_key("x") }.target(),
            None
        );
    }
}
