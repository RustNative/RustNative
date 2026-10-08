//! Input: hardware keys and command shortcuts (`keys`), `MotionEvent`s in
//! the portable pointer model (`pointer`), system back and predictive back
//! on the navigation model (`back`), and the soft keyboard.

pub(crate) mod gamepad;

/// The `PointerIcon` type for a cursor (a mouse or stylus hovering).
pub(crate) const fn pointer_icon(cursor: rustnative_core::Cursor) -> i32 {
    use rustnative_core::Cursor;
    match cursor {
        Cursor::Pointer => 1002,                 // TYPE_HAND
        Cursor::Text => 1008,                    // TYPE_TEXT
        Cursor::Crosshair => 1007,               // TYPE_CROSSHAIR
        Cursor::Move => 1013,                    // TYPE_ALL_SCROLL
        Cursor::NotAllowed => 1012,              // TYPE_NO_DROP
        Cursor::ResizeVertical => 1015,          // TYPE_VERTICAL_DOUBLE_ARROW
        Cursor::ResizeHorizontal => 1014,        // TYPE_HORIZONTAL_DOUBLE_ARROW
        Cursor::Wait | Cursor::Progress => 1004, // TYPE_WAIT
        Cursor::Help => 1003,                    // TYPE_HELP
        Cursor::Default => 1000,                 // TYPE_ARROW
    }
}
pub(crate) mod keys;
pub(crate) mod phases;
pub(crate) mod pointer;

#[cfg(target_os = "android")]
mod back;
#[cfg(target_os = "android")]
pub(crate) mod drag;
#[cfg(target_os = "android")]
pub(crate) use back::{back, sync_back};

use rustnative_core::NodeId;

/// A key event as `RnBridge.nativeKey` hands it over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyFrame {
    /// `KeyEvent.ACTION_DOWN` (0) or `ACTION_UP` (1).
    pub(crate) action: i32,
    pub(crate) key_code: i32,
    pub(crate) meta: i32,
    /// The character the key produces with its modifiers, or 0.
    pub(crate) unicode: i32,
    pub(crate) repeat: i32,
    pub(crate) source: i32,
}

/// A pointer event as `RnBridge.nativePointer` hands it over.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PointerFrame {
    /// `MotionEvent.getActionMasked()`.
    pub(crate) action: i32,
    /// The pointer the action concerns (for pointer down/up).
    pub(crate) action_index: i32,
    pub(crate) ids: Vec<i32>,
    /// `MotionEvent.TOOL_TYPE_*` per pointer.
    pub(crate) tools: Vec<i32>,
    /// Per pointer, `pointer::VALUES` floats: x, y, pressure, tilt,
    /// orientation, horizontal and vertical scroll.
    pub(crate) values: Vec<f32>,
    pub(crate) buttons: i32,
    pub(crate) meta: i32,
    pub(crate) time: i64,
}

/// One window's input state.
#[derive(Debug, Default)]
pub(crate) struct InputState {
    /// The node with input focus.
    pub(crate) focused: Option<NodeId>,
    /// Whether a mouse, trackpad, or pen is the pointer (`keys::POINTER`).
    pub(crate) fine_pointer: bool,
    /// The pointer tracking state (`pointer`).
    pub(crate) pointer: pointer::PointerState,
    /// An input-method composition is in progress for a custom target.
    pub(crate) composing: bool,
    /// What the component under a drag answered it would do with a drop.
    pub(crate) drop_effect: Option<rustnative_core::DropEffect>,
    /// Connected game controllers (`gamepad`).
    pub(crate) gamepads: gamepad::Gamepads,
    /// Whether the soft keyboard is shown for a custom text target.
    pub(crate) keyboard_shown: bool,
}

impl InputState {
    /// Releases what input holds outside the views (pointer capture, a
    /// composition).
    pub(crate) fn release(&mut self) {
        self.pointer = pointer::PointerState::default();
        self.composing = false;
    }
}

#[cfg(target_os = "android")]
pub(crate) use platform::{apply_requests, focus_moved, gamepad_input, key, pointer, text_input};

#[cfg(target_os = "android")]
mod platform {
    use rustnative_core::{Event, WindowId};

    use super::{KeyFrame, PointerFrame};
    use crate::Error;
    use crate::backend::{Work, post};
    use crate::registry::WindowRegistry;

    /// A key: a command shortcut takes it before anything else (`true`: the
    /// focused view must not see it); otherwise the focused node hears it
    /// (after this returns) and the focused view handles it as it would.
    pub(crate) fn key(
        registry: &mut WindowRegistry,
        window: WindowId,
        frame: &KeyFrame,
    ) -> Result<bool, Error> {
        let (key, modifiers) = (
            super::keys::key_code(frame.key_code, frame.unicode),
            super::keys::modifiers(frame.meta),
        );
        let target = registry.windows.get(&window).and_then(|runtime| runtime.input.focused);
        if frame.action == 0 && frame.repeat == 0 {
            let taken = registry.with_application(|application| {
                application.handle_shortcut(window, key, modifiers, target)
            });
            if taken {
                registry.render(window)?;
                registry.after_change(window)?;
                return Ok(true);
            }
        }
        let event = if frame.action == 0 {
            Event::KeyDown { target, key, modifiers }
        } else {
            Event::KeyUp { target, key, modifiers }
        };
        post(Work::Event(window, event));
        Ok(false)
    }

    /// A pointer event, before any view sees it: translated and delivered
    /// to the nodes that declared interest; `true` when the framework
    /// claims the sequence.
    pub(crate) fn pointer(
        registry: &mut WindowRegistry,
        window: WindowId,
        frame: &PointerFrame,
    ) -> Result<bool, Error> {
        super::pointer::offer(registry, window, frame)
    }

    /// A game controller's input; whether a node took it.
    pub(crate) fn gamepad_input(
        registry: &mut WindowRegistry,
        window: WindowId,
        device: i32,
        key_code: i32,
        action: i32,
        axes: Option<&[f32]>,
    ) -> Result<bool, Error> {
        let targets: Vec<rustnative_core::NodeId> = registry
            .windows
            .get(&window)
            .and_then(|runtime| runtime.renderer.as_ref())
            .map(|renderer| {
                renderer
                    .snapshot
                    .nodes()
                    .filter(|node| node.input.wants_gamepad())
                    .map(|node| node.id)
                    .collect()
            })
            .unwrap_or_default();
        if targets.is_empty() {
            return Ok(false);
        }
        let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(false) };
        let inputs = match axes {
            Some(axes) => runtime.input.gamepads.axes(device, axes),
            None => runtime.input.gamepads.key(device, key_code, action == 0),
        };
        for (gamepad, input) in inputs {
            for target in &targets {
                registry.dispatch(window, Event::Gamepad { target: *target, gamepad, input })?;
            }
        }
        Ok(true)
    }

    /// An input-method step for a custom text target (`RnInput`).
    pub(crate) fn text_input(
        registry: &mut WindowRegistry,
        window: WindowId,
        tag: i32,
        step: i32,
        text: String,
        cursor: i32,
    ) -> Result<(), Error> {
        use rustnative_core::Composition;
        let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
        let target = runtime
            .renderer
            .as_ref()
            .and_then(|renderer| renderer.registry.node_for_tag(tag))
            .or(runtime.input.focused);
        let composing = runtime.input.composing;
        let mut events = Vec::new();
        match step {
            1 => {
                if !composing {
                    events.push(Composition::Started);
                }
                runtime.input.composing = true;
                events.push(Composition::Updated {
                    text,
                    cursor: usize::try_from(cursor).unwrap_or(0),
                });
            }
            2 if composing => {
                runtime.input.composing = false;
                events.push(Composition::Committed { text });
            }
            2 => return registry.dispatch(window, Event::TextInput { target, text }),
            _ if composing => {
                runtime.input.composing = false;
                events.push(Composition::Cancelled);
            }
            _ => {}
        }
        for composition in events {
            registry.dispatch(window, Event::Composition { target, composition })?;
        }
        Ok(())
    }

    /// Focus moved to `node`: a framework view declaring itself a text
    /// input (a custom text target) shows the soft keyboard; anything else
    /// hides it if it was shown for one.
    pub(crate) fn focus_moved(
        registry: &mut WindowRegistry,
        window: WindowId,
        node: Option<rustnative_core::NodeId>,
    ) -> Result<(), Error> {
        use crate::jni_host::{Arg, Class, call_static};
        let Some(runtime) = registry.windows.get_mut(&window) else { return Ok(()) };
        let Some(renderer) = runtime.renderer.as_ref() else { return Ok(()) };
        let target = node.and_then(|node| {
            let tree_node = renderer.snapshot.get(node)?;
            let object = renderer.registry.get(node)?;
            let framework =
                matches!(object.java_kind, crate::protocol::CONTAINER | crate::protocol::CANVAS);
            (framework
                && tree_node.accessibility.role() == rustnative_core::AccessibilityRole::TextInput)
                .then(|| object.view.clone())
        });
        if let Some(view) = target {
            call_static(Class::Input, "show", "(Landroid/view/View;)V", &[Arg::Obj(&view)])?;
            runtime.input.keyboard_shown = true;
        } else if std::mem::take(&mut runtime.input.keyboard_shown) {
            if let Some(root) = &runtime.root {
                call_static(Class::Input, "hide", "(Landroid/view/View;)V", &[Arg::Obj(root)])?;
            }
        }
        Ok(())
    }

    /// What components asked of input after a change (focus, the soft
    /// keyboard, text input for a custom target).
    pub(crate) fn apply_requests(
        registry: &mut WindowRegistry,
        window: WindowId,
    ) -> Result<(), Error> {
        let requests =
            registry.with_application(|application| application.take_input_requests(window));
        super::pointer::apply_requests(registry, window, requests)
    }
}
