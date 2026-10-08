//! Foreign views adopted as leaves of the tree (embedding outward, `PLAN.md`
//! Milestone 40), and the host content built on them (Milestone 48): a view
//! the framework did not write, made by a factory, then measured, laid out,
//! clipped, and released by the framework's own rules. Its accessibility is
//! its own.
//!
//! | Host content | View |
//! |---|---|
//! | Web | `WebView` (the system's WebView provider) |
//! | Media | `VideoView` with the host's `MediaController` |
//! | Camera | a Camera2 preview in a `TextureView`, once the camera permission is granted |

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rustnative_core::{HostContent, Size, TreeNode};

use crate::jni_host::{Arg, Class, JavaRef, call_static};
use crate::mappers::NativeView;
use crate::rendering::{HostObject, Shape};
use crate::{Error, NativeContext, protocol};

/// Who owns a foreign view once the framework has adopted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// The framework: removing the node releases its reference, and the
    /// view goes with it.
    Owned,
    /// The application: removing the node takes the view out of the tree,
    /// and the application's own reference keeps it for reuse.
    Borrowed,
}

/// What a foreign factory hands the framework.
#[derive(Debug, Clone)]
pub struct ForeignView {
    /// The view, not yet in any container.
    pub view: NativeView,
    /// Who keeps it.
    pub ownership: Ownership,
}

type Create = Rc<dyn Fn(&NativeView) -> Option<ForeignView>>;

enum Factory {
    Native(Create),
    Class(String),
}

struct Registration {
    preferred: Size,
    factory: Factory,
}

thread_local! {
    static FACTORIES: RefCell<HashMap<String, Registration>> = RefCell::new(HashMap::new());
}

/// Registers the factory for foreign nodes of `kind`. `preferred` is the
/// view's natural size in dp, which layout uses where the node does not fix
/// one; `create` is given the activity (to construct the view with its own
/// JNI calls, through [`NativeView::raw`] and [`crate::java_vm`]) and
/// returns the view, or `None` if it could not make one.
pub fn register_foreign(
    kind: impl Into<String>,
    preferred: Size,
    create: impl Fn(&NativeView) -> Option<ForeignView> + 'static,
) {
    FACTORIES.with(|factories| {
        factories.borrow_mut().insert(
            kind.into(),
            Registration { preferred, factory: Factory::Native(Rc::new(create)) },
        )
    });
}

/// Registers `class_name` (a `View` subclass with a `(Context)`
/// constructor, `android.widget.CalendarView` say) as the factory for
/// foreign nodes of `kind`.
pub fn register_foreign_class(
    kind: impl Into<String>,
    preferred: Size,
    class_name: impl Into<String>,
) {
    FACTORIES.with(|factories| {
        factories.borrow_mut().insert(
            kind.into(),
            Registration { preferred, factory: Factory::Class(class_name.into()) },
        )
    });
}

/// A foreign kind's preferred size, in dp, if its factory states one.
pub(crate) fn preferred_size(kind: &str, _density: f32) -> Option<Size> {
    if let Some(content) = HostContent::from_kind(kind) {
        return Some(match content {
            HostContent::Web { .. } => Size::new(320, 240),
            HostContent::Media { .. } => Size::new(320, 180),
            HostContent::Camera { .. } => Size::new(240, 320),
        });
    }
    FACTORIES
        .with(|factories| factories.borrow().get(kind).map(|registration| registration.preferred))
}

/// Creates the view a foreign node of `kind` is realized as.
pub(crate) fn create(
    activity: &JavaRef,
    kind: &str,
    node: &TreeNode,
    window: u64,
    tag: i32,
) -> Result<HostObject, Error> {
    let unavailable = |reason| Error::ForeignUnavailable {
        kind: kind.to_owned(),
        reason,
        context: NativeContext::none().with_node(node.id),
    };
    let view = if let Some(content) = HostContent::from_kind(kind) {
        let made = match content {
            HostContent::Web { url } => call_static(
                Class::Host,
                "web",
                "(Landroid/app/Activity;Ljava/lang/String;)Landroid/view/View;",
                &[Arg::Obj(activity), Arg::Str(&url)],
            )?,
            HostContent::Media { source } => call_static(
                Class::Host,
                "media",
                "(Landroid/app/Activity;Ljava/lang/String;)Landroid/view/View;",
                &[Arg::Obj(activity), Arg::Str(&source)],
            )?,
            HostContent::Camera { device } => call_static(
                Class::Host,
                "camera",
                "(Landroid/app/Activity;I)Landroid/view/View;",
                &[Arg::Obj(activity), Arg::Int(i32::try_from(device).unwrap_or(0))],
            )?,
        };
        made.obj().ok_or_else(|| unavailable("the host made no view for it"))?
    } else {
        let factory = FACTORIES.with(|factories| {
            factories.borrow().get(kind).map(|registration| match &registration.factory {
                Factory::Native(create) => Factory::Native(Rc::clone(create)),
                Factory::Class(name) => Factory::Class(name.clone()),
            })
        });
        match factory {
            Some(Factory::Native(create)) => {
                create(&NativeView(activity.clone()))
                    .ok_or_else(|| unavailable("its factory made no view"))?
                    .view
                    .0
            }
            Some(Factory::Class(name)) => call_static(
                Class::Host,
                "byClass",
                "(Landroid/app/Activity;Ljava/lang/String;)Landroid/view/View;",
                &[Arg::Obj(activity), Arg::Str(&name)],
            )?
            .obj()
            .ok_or_else(|| unavailable("its class could not be made"))?,
            None => return Err(unavailable("no factory is registered for this kind")),
        }
    };
    call_static(
        Class::Host,
        "adopt",
        "(Landroid/view/View;JI)V",
        &[Arg::Obj(&view), Arg::Long(i64::try_from(window).unwrap_or(0)), Arg::Int(tag)],
    )?;
    Ok(HostObject {
        kind: node.kind,
        java_kind: protocol::FOREIGN,
        view,
        content: None,
        shape: Shape::Foreign(kind.to_owned()),
        tag,
        style: None,
        drop: false,
        cursor: None,
    })
}
