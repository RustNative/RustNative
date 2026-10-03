//! The GTK backend's answers to the inspection protocol (`PLAN.md`
//! Milestone 44), and the in-application overlay.
//!
//! The realized objects are the renderer's registry — each with its GTK
//! type and its rectangle as GTK allocated it — so the mapping between the
//! declarative tree and the widgets is read, not reconstructed.
//!
//! The overlay is a canvas (the widget a `Node::canvas` draws with) laid
//! over the whole window as the root container's last child, so it draws
//! above everything; it takes no input, so the window works under it.

use std::collections::HashMap;

use gtk::prelude::*;
use rustnative_core::inspect::{InspectBackend, Lifetimes, RealizedObject, node_name};
use rustnative_core::{NodeId, Platform as _, PlatformCapabilities, Rect, WindowId};

use super::canvas::RnCanvas;
use super::registry::WindowRuntime;

/// One window's renderer, answering for that window.
pub(crate) struct GtkInspect<'a> {
    pub(crate) runtime: &'a WindowRuntime,
}

impl GtkInspect<'_> {
    /// Every laid-out node at its window rectangle, as GTK placed it.
    fn window_rects(&self) -> HashMap<NodeId, Rect> {
        let root = self.runtime.root.upcast_ref::<gtk::Widget>();
        self.runtime
            .renderer
            .registry
            .iter()
            .filter_map(|(id, object)| {
                let bounds = object.widget.compute_bounds(root)?;
                #[allow(clippy::cast_possible_truncation, reason = "pixel bounds, rounded")]
                let rect = Rect::new(
                    bounds.x().round() as i32,
                    bounds.y().round() as i32,
                    bounds.width().round() as i32,
                    bounds.height().round() as i32,
                );
                Some((id, rect))
            })
            .collect()
    }
}

impl InspectBackend for GtkInspect<'_> {
    fn name(&self) -> &'static str {
        "linux-gtk4"
    }

    fn realized(&self, window: WindowId) -> Vec<RealizedObject> {
        if window != self.runtime.id {
            return Vec::new();
        }
        let mut objects: Vec<RealizedObject> = self
            .runtime
            .renderer
            .registry
            .iter()
            .map(|(id, object)| RealizedObject {
                node: node_name(id),
                key: id.local_key(),
                host_type: object.widget.type_().name().to_owned(),
                handle: Some(format!("{:#x}", object.widget.as_ptr() as usize)),
                rect: object
                    .widget
                    .parent()
                    .and_then(|parent| object.widget.compute_bounds(&parent))
                    .map(|bounds| {
                        #[allow(clippy::cast_possible_truncation, reason = "pixel bounds, rounded")]
                        [
                            bounds.x().round() as i32,
                            bounds.y().round() as i32,
                            bounds.width().round() as i32,
                            bounds.height().round() as i32,
                        ]
                    }),
            })
            .collect();
        objects.sort_by(|a, b| a.node.cmp(&b.node));
        objects
    }

    fn rects(&self, window: WindowId) -> Option<HashMap<NodeId, Rect>> {
        (window == self.runtime.id).then(|| self.runtime.renderer.layout_rects().clone())
    }

    fn lifetimes(&self) -> Lifetimes {
        let registry = &self.runtime.renderer.registry;
        Lifetimes {
            created: registry.created,
            destroyed: registry.destroyed,
            live: registry.created.saturating_sub(registry.destroyed),
            recent: Vec::new(),
        }
    }

    fn capabilities(&self) -> PlatformCapabilities {
        crate::LinuxPlatform::new().capabilities()
    }

    fn style_capabilities(&self) -> rustnative_style::StyleCapabilities {
        rustnative_style::LINUX
    }

    fn unit_mapping(&self) -> Option<rustnative_style::UnitMapping> {
        Some(rustnative_style::LINUX_UNITS)
    }

    fn mappers(&self) -> Vec<rustnative_core::inspect::MapperEntry> {
        crate::mappers::active_mappers()
            .into_iter()
            .map(|mapper| rustnative_core::inspect::MapperEntry {
                target: match mapper.target {
                    crate::MapperTarget::Kind(kind) => format!("every {kind:?}"),
                    crate::MapperTarget::Key(key) => format!("the node `{key}`"),
                },
                property: format!("{:?}", mapper.property),
                mode: format!("{:?}", mapper.mode).to_lowercase(),
            })
            .collect()
    }
}

/// Answers waiting inspection requests for `runtime`'s window, closing it
/// if the inspector asked the application to quit. Returns whether any
/// request was answered.
pub(crate) fn poll(
    runtime: &WindowRuntime,
    application: &mut rustnative_core::Application,
) -> bool {
    let answered = application.poll_inspection(&GtkInspect { runtime });
    // Asked to close (the development loop restarting it): closed as the
    // person closing the window would, so state is flushed and placement
    // saved.
    if application.take_quit_request() {
        if let Some(window) = &runtime.window {
            window.close();
        }
    }
    answered
}

/// Shows, redraws, or removes `runtime`'s overlay to match the
/// application's overlay mode.
pub(crate) fn sync_overlay(
    runtime: &mut WindowRuntime,
    application: &mut rustnative_core::Application,
) {
    let rects = GtkInspect { runtime }.window_rects();
    let list = application.overlay_draw_list(runtime.id, &rects);
    match (list, &runtime.overlay) {
        (None, None) => {}
        (None, Some(overlay)) => {
            runtime.root.remove(overlay.upcast_ref::<gtk::Widget>());
            runtime.overlay = None;
        }
        (Some(list), existing) => {
            let overlay = existing.clone().unwrap_or_else(|| {
                let overlay = RnCanvas::create(gtk::AccessibleRole::Presentation);
                overlay.set_can_target(false);
                overlay.set_can_focus(false);
                runtime.root.append(overlay.upcast_ref::<gtk::Widget>());
                overlay
            });
            let (width, height) = (runtime.root.width(), runtime.root.height());
            runtime.root.place(overlay.upcast_ref::<gtk::Widget>(), Rect::new(0, 0, width, height));
            overlay.set_draw_list(&list);
            runtime.overlay = Some(overlay);
        }
    }
}
