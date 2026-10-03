//! Native input: GDK's keyboard, pointer, touch, pen, scroll, input-method,
//! clipboard, and drag-and-drop events translated into the portable model.
//!
//! Each window gets its controllers once, on the window itself:
//!
//! | Controller | Phase | Does |
//! |---|---|---|
//! | `GtkEventControllerKey` | capture | command shortcuts first (they win over the focused widget, as an accelerator does), then `KeyDown`/`KeyUp` to the focused node; custom text targets get an input method |
//! | `GtkEventControllerLegacy` | capture | pointer, touch, and pen samples, hover, and wheel for nodes that declared interest; never consumes what a native widget needs |
//! | `notify::focus-widget` | — | `FocusGained`/`FocusLost` |
//! | the display's `GdkClipboard` | — | `ClipboardChanged` |
//!
//! Tab traversal is GTK's own: focusable widgets are in declarative order
//! (the renderer keeps them so), and GTK moves focus the way every GTK
//! application does.

mod drop;
pub(crate) mod keys;
pub(crate) mod pointer;

use std::collections::HashMap;
use std::time::Instant;

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{
    ClipboardAction, Composition, Event, GestureRecognizer, InputRequest, KeyCode, KeyModifiers,
    NodeId, WindowId,
};

pub(crate) use drop::sync_drop_target;

use super::backend::{Work, answer, post};
use super::registry::WindowRegistry;

/// One window's input bookkeeping.
pub(crate) struct InputState {
    pub(crate) epoch: Instant,
    pub(crate) hover: Option<NodeId>,
    pub(crate) captures: HashMap<u32, pointer::Capture>,
    pub(crate) recognizers: HashMap<NodeId, GestureRecognizer>,
    /// Touch sequences' pointer ids, by sequence.
    pub(crate) sequences: HashMap<usize, u32>,
    pub(crate) next_sequence_id: u32,
    pub(crate) focused: Option<NodeId>,
    /// The answer to the drag in progress.
    pub(crate) drop_effect: rustnative_core::DropEffect,
    /// The key controller, whose input method is switched on for custom
    /// text targets and off for GTK's own text widgets (which have theirs).
    key_controller: Option<gtk::EventControllerKey>,
    im: Option<gtk::IMMulticontext>,
    composing: bool,
    clipboard: Option<(gtk::gdk::Clipboard, glib::SignalHandlerId)>,
    pub(crate) long_press: Option<glib::SourceId>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            hover: None,
            captures: HashMap::new(),
            recognizers: HashMap::new(),
            sequences: HashMap::new(),
            next_sequence_id: 1,
            focused: None,
            drop_effect: rustnative_core::DropEffect::None,
            key_controller: None,
            im: None,
            composing: false,
            clipboard: None,
            long_press: None,
        }
    }
}

impl std::fmt::Debug for InputState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputState")
            .field("focused", &self.focused)
            .field("hover", &self.hover)
            .finish_non_exhaustive()
    }
}

impl InputState {
    /// Releases what outlives the window: the clipboard handler and the
    /// long-press timer.
    pub(crate) fn release(&mut self) {
        if let Some((clipboard, handler)) = self.clipboard.take() {
            clipboard.disconnect(handler);
        }
        if let Some(source) = self.long_press.take() {
            source.remove();
        }
        if let Some(im) = &self.im {
            im.set_client_widget(None::<&gtk::Widget>);
        }
    }
}

/// Installs a window's controllers on `host`: its `GtkWindow`, or — for a
/// root embedded in a host application's widget tree — the root itself,
/// which then hears the input that reaches it there.
pub(crate) fn attach(
    host: &gtk::Widget,
    root: &super::layout_widget::RnLayout,
    id: WindowId,
) -> InputState {
    let window = host;
    let mut state = InputState::default();

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let display = WidgetExt::display(window);
    keys.connect_key_pressed(move |controller, keyval, keycode, modifier| {
        let group = controller
            .current_event()
            .and_then(|event| event.downcast::<gtk::gdk::KeyEvent>().ok())
            .map_or(0, |event| event.layout());
        let key = keys::key_code(keyval, keys::unshifted(&display, keycode, group));
        let held = keys::modifiers(modifier);
        if key == KeyCode::Tab {
            // GTK's own focus traversal.
            return glib::Propagation::Proceed;
        }
        let shortcut = answer(id, |registry| registry.shortcut(id, key, held)).unwrap_or(false);
        if shortcut {
            return glib::Propagation::Stop;
        }
        post(Work::Key { window: id, key, modifiers: held, pressed: true });
        glib::Propagation::Proceed
    });
    let display = WidgetExt::display(window);
    keys.connect_key_released(move |controller, keyval, keycode, modifier| {
        let group = controller
            .current_event()
            .and_then(|event| event.downcast::<gtk::gdk::KeyEvent>().ok())
            .map_or(0, |event| event.layout());
        let key = keys::key_code(keyval, keys::unshifted(&display, keycode, group));
        if key != KeyCode::Tab {
            post(Work::Key {
                window: id,
                key,
                modifiers: keys::modifiers(modifier),
                pressed: false,
            });
        }
    });
    let im = gtk::IMMulticontext::new();
    im.set_client_widget(Some(window));
    im.connect_commit(move |_, text| post(Work::Text { window: id, text: text.to_owned() }));
    im.connect_preedit_start(move |_| {
        post(Work::Composition { window: id, composition: Composition::Started });
    });
    im.connect_preedit_changed(move |im| {
        let (text, _, cursor) = im.preedit_string();
        let cursor = usize::try_from(cursor.max(0)).unwrap_or(0);
        post(Work::Composition {
            window: id,
            composition: Composition::Updated { text: text.to_string(), cursor },
        });
    });
    im.connect_preedit_end(move |_| {
        post(Work::Composition { window: id, composition: Composition::Cancelled });
    });
    window.add_controller(keys.clone());
    state.key_controller = Some(keys);
    state.im = Some(im);

    let pointer = gtk::EventControllerLegacy::new();
    pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
    let origin = root.clone();
    pointer.connect_event(move |controller, event| {
        // The surface the event's coordinates are in: the window's, wherever
        // the host put the root.
        let Some(native) = controller.widget().and_then(|widget| widget.native()) else {
            return glib::Propagation::Proceed;
        };
        pointer::legacy_event(id, &native, &origin, event)
    });
    window.add_controller(pointer);

    // Focus moves within the toplevel the root is in (the host's, when
    // embedded), known once the root is realized.
    match window.downcast_ref::<gtk::Window>() {
        Some(toplevel) => {
            toplevel.connect_focus_widget_notify(move |_| post(Work::FocusChanged(id)));
        }
        None => {
            window.connect_realize(move |widget| {
                if let Some(toplevel) = widget.root().and_downcast::<gtk::Window>() {
                    toplevel.connect_focus_widget_notify(move |_| post(Work::FocusChanged(id)));
                }
            });
        }
    }

    let clipboard = window.clipboard();
    let handler = clipboard
        .connect_changed(move |_| post(Work::Event(id, Event::ClipboardChanged { window: id })));
    state.clipboard = Some((clipboard, handler));
    state
}

impl WindowRegistry {
    /// Offers a key press to the window's command shortcuts; whether one
    /// took it.
    pub(crate) fn shortcut(
        &mut self,
        window: WindowId,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<bool, crate::Error> {
        let focused = self.windows.get(&window).and_then(|runtime| runtime.input.focused);
        let handled = self.with_application(|application| {
            application.handle_shortcut(window, key, modifiers, focused)
        });
        if handled {
            self.render(window)?;
            self.sync()?;
        }
        Ok(handled)
    }

    /// A key reached the window (and was not a shortcut).
    pub(crate) fn key(
        &mut self,
        window: WindowId,
        key: KeyCode,
        modifiers: KeyModifiers,
        pressed: bool,
    ) -> Result<(), crate::Error> {
        let target = self.windows.get(&window).and_then(|runtime| runtime.input.focused);
        if pressed {
            self.dispatch(window, Event::KeyDown { target, key, modifiers })?;
            if let Some(shortcut) = clipboard_shortcut(key, modifiers) {
                self.clipboard_shortcut(window, target, shortcut);
            }
            Ok(())
        } else {
            self.dispatch(window, Event::KeyUp { target, key, modifiers })
        }
    }

    /// Committed text for a custom text target.
    pub(crate) fn text(&mut self, window: WindowId, text: String) -> Result<(), crate::Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        let target = runtime.input.focused;
        if runtime.input.composing {
            runtime.input.composing = false;
            self.dispatch(
                window,
                Event::Composition { target, composition: Composition::Committed { text } },
            )
        } else {
            self.dispatch(window, Event::TextInput { target, text })
        }
    }

    /// An input-method composition step for a custom text target.
    pub(crate) fn composition(
        &mut self,
        window: WindowId,
        composition: Composition,
    ) -> Result<(), crate::Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        match composition {
            Composition::Started => runtime.input.composing = true,
            // A preedit that ends by committing reports `Committed` from
            // the commit; one that ends without is a cancellation.
            Composition::Cancelled if !runtime.input.composing => return Ok(()),
            Composition::Cancelled => runtime.input.composing = false,
            _ => {}
        }
        let target = runtime.input.focused;
        self.dispatch(window, Event::Composition { target, composition })
    }

    /// Reports a focus change as the portable events, and switches the
    /// input method to match what now has focus.
    pub(crate) fn focus_changed(&mut self, window: WindowId) -> Result<(), crate::Error> {
        let Some(runtime) = self.windows.get_mut(&window) else { return Ok(()) };
        let focus_widget = runtime.window.as_ref().and_then(gtk::prelude::GtkWindowExt::focus);
        let next = focus_widget
            .as_ref()
            .and_then(|widget| runtime.renderer.registry.node_for_widget(widget));
        let native_text = focus_widget.as_ref().is_some_and(|widget| {
            widget.is::<gtk::Text>()
                || widget.is::<gtk::TextView>()
                || widget.is::<gtk::Entry>()
                || widget.is::<gtk::SpinButton>()
        });
        if let (Some(controller), Some(im)) = (&runtime.input.key_controller, &runtime.input.im) {
            if native_text {
                controller.set_im_context(None::<&gtk::IMContext>);
                im.focus_out();
            } else {
                controller.set_im_context(Some(im));
                im.focus_in();
            }
        }
        let previous = runtime.input.focused;
        if previous == next {
            return Ok(());
        }
        runtime.input.focused = next;
        if let Some(previous) = previous {
            self.dispatch(window, Event::FocusLost { target: previous })?;
        }
        if let Some(next) = next {
            self.dispatch(window, Event::FocusGained { target: next })?;
        }
        Ok(())
    }

    /// Applies the pointer-capture and drag-feedback requests components
    /// made during the dispatch that just finished.
    pub(crate) fn apply_input_requests(&mut self, window: WindowId) {
        let requests = self.with_application(|application| application.take_input_requests(window));
        let Some(runtime) = self.windows.get_mut(&window) else { return };
        for request in requests {
            match request {
                InputRequest::SetDropEffect(effect) => runtime.input.drop_effect = effect,
                other => pointer::apply_request(runtime, other),
            }
        }
    }

    fn clipboard_shortcut(
        &mut self,
        window: WindowId,
        target: Option<NodeId>,
        shortcut: ClipboardShortcut,
    ) {
        let action = match shortcut {
            ClipboardShortcut::Copy => ClipboardAction::Copy,
            ClipboardShortcut::Cut => ClipboardAction::Cut,
            ClipboardShortcut::Paste => {
                // GDK reads the clipboard asynchronously; the paste is
                // reported with its text once it has arrived.
                let Some(clipboard) = self
                    .windows
                    .get(&window)
                    .and_then(|runtime| runtime.window.clone())
                    .map(|w| w.clipboard())
                else {
                    return;
                };
                clipboard.read_text_async(gtk::gio::Cancellable::NONE, move |text| {
                    let text = text.ok().flatten().map(|text| text.to_string());
                    post(Work::Event(
                        window,
                        Event::Clipboard { target, action: ClipboardAction::Paste { text } },
                    ));
                });
                return;
            }
        };
        let _ = self.dispatch(window, Event::Clipboard { target, action });
    }
}

/// The clipboard operation a key press performs: Ctrl+C/X/V, and the CUA
/// equivalents (Ctrl+Insert, Shift+Delete, Shift+Insert) GTK's own text
/// widgets also honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardShortcut {
    Copy,
    Cut,
    Paste,
}

pub(crate) fn clipboard_shortcut(
    key: KeyCode,
    modifiers: KeyModifiers,
) -> Option<ClipboardShortcut> {
    let only_ctrl = modifiers.ctrl && !modifiers.shift && !modifiers.alt && !modifiers.meta;
    let only_shift = modifiers.shift && !modifiers.ctrl && !modifiers.alt && !modifiers.meta;
    match key {
        KeyCode::Character('C') | KeyCode::Insert if only_ctrl => Some(ClipboardShortcut::Copy),
        KeyCode::Character('X') if only_ctrl => Some(ClipboardShortcut::Cut),
        KeyCode::Delete if only_shift => Some(ClipboardShortcut::Cut),
        KeyCode::Character('V') if only_ctrl => Some(ClipboardShortcut::Paste),
        KeyCode::Insert if only_shift => Some(ClipboardShortcut::Paste),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_shortcuts_include_the_cua_keys() {
        let ctrl = KeyModifiers { ctrl: true, ..KeyModifiers::default() };
        let shift = KeyModifiers { shift: true, ..KeyModifiers::default() };
        assert_eq!(
            clipboard_shortcut(KeyCode::Character('C'), ctrl),
            Some(ClipboardShortcut::Copy)
        );
        assert_eq!(clipboard_shortcut(KeyCode::Insert, ctrl), Some(ClipboardShortcut::Copy));
        assert_eq!(clipboard_shortcut(KeyCode::Delete, shift), Some(ClipboardShortcut::Cut));
        assert_eq!(clipboard_shortcut(KeyCode::Insert, shift), Some(ClipboardShortcut::Paste));
        assert_eq!(clipboard_shortcut(KeyCode::Character('V'), shift), None);
    }
}
