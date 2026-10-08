//! The Android backend's answers to the inspection protocol (`PLAN.md`
//! Milestone 44), and the in-application overlay.
//!
//! Inspection starts when the launch intent carries
//! `dev.rustnative.inspect` (`rustnative run android --inspect` adds it):
//! the server listens on the device's loopback on [`PORT`], the endpoint
//! (with its token) is written to `getFilesDir()/rustnative/inspect.json`,
//! and `rustnative inspect --android` forwards the port with `adb forward`
//! and reads the endpoint with `run-as` (a debuggable build).
//!
//! The realized objects are the renderer's registry — each with its Java
//! class and identity hash code, and its rectangle as Android placed it —
//! so the mapping between the declarative tree and the views is read, not
//! reconstructed. The overlay is an `RnCanvasView` laid over the window as
//! its root's last child, taking no input.

use std::collections::HashMap;

use rustnative_core::inspect::{InspectBackend, Lifetimes, RealizedObject, node_name};
use rustnative_core::{NodeId, Platform as _, PlatformCapabilities, Rect, WindowId};

use crate::jni_host::{Arg, Class, JavaRef, Ret, call_static};
use crate::registry::{WindowRegistry, WindowRuntime};

/// The device-side port the inspection server listens on.
pub(crate) const PORT: u16 = 7920;

/// One window's renderer, answering for that window.
struct AndroidInspect<'a> {
    runtime: &'a WindowRuntime,
}

/// `view`'s bounds in `root`, in pixels.
fn bounds(view: &JavaRef, root: &JavaRef) -> Option<[i32; 4]> {
    let values = call_static(
        Class::Views,
        "boundsIn",
        "(Landroid/view/View;Landroid/view/View;)[I",
        &[Arg::Obj(view), Arg::Obj(root)],
    )
    .ok()
    .map(Ret::ints)?;
    <[i32; 4]>::try_from(values.as_slice()).ok()
}

impl AndroidInspect<'_> {
    /// Every realized node at its window rectangle (dp), as Android placed it.
    fn window_rects(&self) -> HashMap<NodeId, Rect> {
        let (Some(renderer), Some(root)) = (&self.runtime.renderer, &self.runtime.root) else {
            return HashMap::new();
        };
        let density = self.runtime.density.max(0.1);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            reason = "pixels over the density, rounded"
        )]
        let dp = |pixels: i32| (pixels as f32 / density).round() as i32;
        renderer
            .registry
            .iter()
            .filter_map(|(id, object)| {
                let [x, y, width, height] = bounds(&object.view, root)?;
                Some((id, Rect::new(dp(x), dp(y), dp(width), dp(height))))
            })
            .collect()
    }
}

impl InspectBackend for AndroidInspect<'_> {
    fn name(&self) -> &'static str {
        "android-views"
    }

    fn realized(&self, window: WindowId) -> Vec<RealizedObject> {
        let Some(renderer) =
            (window == self.runtime.id).then_some(()).and(self.runtime.renderer.as_ref())
        else {
            return Vec::new();
        };
        let mut objects: Vec<RealizedObject> = renderer
            .registry
            .iter()
            .map(|(id, object)| {
                let description = call_static(
                    Class::Views,
                    "identify",
                    "(Landroid/view/View;)[Ljava/lang/String;",
                    &[Arg::Obj(&object.view)],
                )
                .map(Ret::strings)
                .unwrap_or_default();
                let frame = crate::rendering::controls::frame(&object.view);
                RealizedObject {
                    node: node_name(id),
                    key: id.local_key(),
                    host_type: description.first().cloned().unwrap_or_default(),
                    handle: description.get(1).cloned(),
                    rect: Some(frame),
                }
            })
            .collect();
        objects.sort_by(|a, b| a.node.cmp(&b.node));
        objects
    }

    fn rects(&self, window: WindowId) -> Option<HashMap<NodeId, Rect>> {
        (window == self.runtime.id).then(|| self.window_rects())
    }

    fn lifetimes(&self) -> Lifetimes {
        let Some(renderer) = &self.runtime.renderer else { return Lifetimes::default() };
        let registry = &renderer.registry;
        Lifetimes {
            created: registry.created,
            destroyed: registry.destroyed,
            live: registry.created.saturating_sub(registry.destroyed),
            recent: Vec::new(),
        }
    }

    fn capabilities(&self) -> PlatformCapabilities {
        crate::AndroidPlatform::new().capabilities()
    }

    fn style_capabilities(&self) -> rustnative_style::StyleCapabilities {
        rustnative_style::ANDROID
    }

    fn unit_mapping(&self) -> Option<rustnative_style::UnitMapping> {
        Some(rustnative_style::ANDROID_UNITS)
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

/// Answers the inspector's pending requests for `window`, finishing the
/// activity if the inspector asked the application to quit; whether any
/// request was answered (the tree may have changed). Then brings the
/// overlay in line with the application's overlay mode.
pub(crate) fn poll(registry: &mut WindowRegistry, window: WindowId) -> bool {
    let Some(runtime) = registry.windows.remove(&window) else { return false };
    let (answered, quit) = registry.with_application(|application| {
        let answered = application.poll_inspection(&AndroidInspect { runtime: &runtime });
        (answered, application.take_quit_request())
    });
    let rects = AndroidInspect { runtime: &runtime }.window_rects();
    let list =
        registry.with_application(|application| application.overlay_draw_list(window, &rects));
    registry.windows.insert(window, runtime);
    if let Some(runtime) = registry.windows.get_mut(&window) {
        if let Err(error) = sync_overlay(runtime, list.as_ref()) {
            crate::log::warn(&format!("the inspection overlay: {error}"));
        }
        if quit {
            if let Some(activity) = &runtime.activity {
                let _ = call_static(
                    Class::Bridge,
                    "finish",
                    "(Landroid/app/Activity;)V",
                    &[Arg::Obj(activity)],
                );
            }
        }
    }
    answered
}

/// Shows, redraws, or removes the overlay.
fn sync_overlay(
    runtime: &mut WindowRuntime,
    list: Option<&rustnative_core::DrawList>,
) -> Result<(), crate::Error> {
    let Some(root) = runtime.root.clone() else { return Ok(()) };
    match (list, runtime.overlay.take()) {
        (None, None) => {}
        (None, Some(overlay)) => {
            call_static(
                Class::Views,
                "removeOverlay",
                "(Landroid/view/View;Landroid/view/View;)V",
                &[Arg::Obj(&root), Arg::Obj(&overlay)],
            )?;
        }
        (Some(list), existing) => {
            let bytes = crate::canvas::encode(list);
            let existing_arg = existing.as_ref().map_or(Arg::Null, Arg::Obj);
            runtime.overlay = call_static(
                Class::Views,
                "overlay",
                "(Landroid/view/View;Landroid/view/View;[B)Landroid/view/View;",
                &[Arg::Obj(&root), existing_arg, Arg::Bytes(&bytes)],
            )?
            .obj();
        }
    }
    Ok(())
}

/// Starts the inspection server when the launch intent asks for it, and
/// writes the endpoint where `rustnative inspect --android` reads it.
pub(crate) fn enable_from_intent(
    application: &mut rustnative_core::Application,
    intent: Option<&JavaRef>,
) {
    let Some(intent) = intent else { return };
    let asked =
        call_static(Class::Intents, "inspect", "(Landroid/content/Intent;)Z", &[Arg::Obj(intent)])
            .is_ok_and(Ret::bool);
    if !asked {
        return;
    }
    let bind = std::net::SocketAddr::from(([127, 0, 0, 1], PORT));
    match application.enable_inspection(Some(bind)) {
        Ok(endpoint) => {
            let written = crate::services::files_dir()
                .map(|files| files.join("rustnative"))
                .and_then(|directory| {
                    std::fs::create_dir_all(&directory)
                        .and_then(|()| {
                            std::fs::write(
                                directory.join("inspect.json"),
                                serde_json::to_vec(&endpoint).unwrap_or_default(),
                            )
                        })
                        .map_err(|error| rustnative_core::ServiceError::new(error.to_string()))
                });
            match written {
                Ok(()) => crate::log::info(&format!(
                    "inspection listening on {} (rustnative inspect --android)",
                    endpoint.addr
                )),
                Err(error) => crate::log::warn(&format!(
                    "inspection is listening, but its endpoint was not written: {error}"
                )),
            }
        }
        Err(error) => crate::log::error(&format!("inspection could not start: {error}")),
    }
}
