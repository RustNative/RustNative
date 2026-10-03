//! Drop targets: a node that declared `drop_target` interest gets a
//! `GtkDropTarget` for files and text, and the drag negotiates through the
//! component — `DragEnter`/`DragOver` answered with
//! `ComponentContext::input().set_drop_effect`, then `Drop` or `DragLeave`.

use gtk::gdk;
use gtk::prelude::*;
use rustnative_core::{DragData, DropEffect, Event, Point, TreeNode, WindowId};

use super::super::backend::{Work, answer, post};
use super::super::registry::WindowRegistry;
use super::super::rendering::HostObject;

/// What GTK's drop value carries, as the portable drag data.
fn drag_data(value: Option<&gtk::glib::Value>) -> DragData {
    let Some(value) = value else { return DragData::new() };
    if let Ok(files) = value.get::<gdk::FileList>() {
        return DragData::new()
            .with_files(files.files().into_iter().filter_map(|file| file.path()));
    }
    if let Ok(text) = value.get::<String>() {
        return DragData::new().with_text(text);
    }
    DragData::new()
}

const fn action_of(effect: DropEffect) -> gdk::DragAction {
    match effect {
        DropEffect::Copy => gdk::DragAction::COPY,
        DropEffect::Move => gdk::DragAction::MOVE,
        DropEffect::Link => gdk::DragAction::LINK,
        DropEffect::None => gdk::DragAction::empty(),
    }
}

#[allow(clippy::cast_possible_truncation, reason = "a pixel coordinate inside one window")]
fn point(x: f64, y: f64) -> Point {
    Point::new(x.floor() as i32, y.floor() as i32)
}

/// Gives `object` a drop target when `node` wants drops, and takes it away
/// when it no longer does.
pub(crate) fn sync_drop_target(object: &mut HostObject, node: &TreeNode, window: WindowId) {
    let wants = node.input.wants_drop();
    match (&object.drop, wants) {
        (None, true) => {
            let target = gtk::DropTarget::new(
                gtk::glib::Type::INVALID,
                gdk::DragAction::COPY | gdk::DragAction::MOVE | gdk::DragAction::LINK,
            );
            target.set_types(&[gdk::FileList::static_type(), String::static_type()]);
            // The value is loaded on enter, so the component sees what is
            // being dragged before deciding.
            target.set_preload(true);
            let id = node.id;
            target.connect_enter(move |target, x, y| {
                let data = drag_data(target.value().as_ref());
                negotiate(window, Event::DragEnter { target: id, data, position: point(x, y) })
            });
            target.connect_motion(move |target, x, y| {
                let data = drag_data(target.value().as_ref());
                negotiate(window, Event::DragOver { target: id, data, position: point(x, y) })
            });
            target
                .connect_leave(move |_| post(Work::Event(window, Event::DragLeave { target: id })));
            target.connect_drop(move |_, value, x, y| {
                let data = drag_data(Some(value));
                if data.is_empty() {
                    return false;
                }
                post(Work::Event(window, Event::Drop { target: id, data, position: point(x, y) }));
                true
            });
            object.widget.add_controller(target.clone());
            object.drop = Some(target);
        }
        (Some(target), false) => {
            object.widget.remove_controller(target);
            object.drop = None;
        }
        _ => {}
    }
}

/// Delivers a drag step and answers GTK with the component's effect.
fn negotiate(window: WindowId, event: Event) -> gdk::DragAction {
    answer(window, |registry| registry.drag(window, event))
        .map_or(gdk::DragAction::empty(), action_of)
}

impl WindowRegistry {
    /// Delivers a drag step; the effect the component answered with.
    pub(crate) fn drag(
        &mut self,
        window: WindowId,
        event: Event,
    ) -> Result<DropEffect, crate::Error> {
        if let Some(runtime) = self.windows.get_mut(&window) {
            runtime.input.drop_effect = DropEffect::None;
        }
        self.dispatch(window, event)?;
        Ok(self.windows.get(&window).map_or(DropEffect::None, |runtime| runtime.input.drop_effect))
    }
}
