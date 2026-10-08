//! Creating, updating, and wiring the view that realizes each node kind
//! (`docs/android/widget-mapping.md`).
//!
//! | Node | Android view |
//! |---|---|
//! | Column, Row | `RnLayout`; inside a `ScrollView`/`HorizontalScrollView` when it scrolls |
//! | Label | `TextView` |
//! | Button | `Button` |
//! | `TextInput` | single-line `EditText` |
//! | `TabBar` | `RnTabs`: a row of tabs reported as `TabWidget` |
//! | Checkbox | `CheckBox` |
//! | Radio | `RadioButton` (exclusive through the framework) |
//! | Toggle | `Switch` |
//! | Slider | `SeekBar` |
//! | Progress | horizontal `ProgressBar` (indeterminate while unknown) |
//! | Select | `Spinner` over an `ArrayAdapter` |
//! | `ListBox` | single-choice `ListView` |
//! | `DatePicker` | a `Button` showing the date, opening `DatePickerDialog` |
//! | Spinner | `RnStepper`: −, a numeric `EditText`, + |
//! | Separator | a `View` with the theme's list divider |
//! | Link | an underlined `TextView` in the theme's link colour |
//! | `MultilineText` | multi-line `EditText` |
//! | Image | `ImageView` over a `Bitmap` |
//! | Canvas | `RnCanvasView` |
//! | Surface | `RnSurfaceView` (a `SurfaceView`) |
//!
//! The views' listeners are wired in Java (`RnViews.create`) and report
//! through `RnBridge.nativeViewEvent` with the view's tag; the backend
//! mutes them around its own changes (`RnBridge.muted`).

use rustnative_core::{Control, NodeKind, Overflow, TreeNode};

use super::{HostObject, Shape};
use crate::Error;
use crate::jni_host::{Arg, Class, JavaRef, call_static};
use crate::protocol;

const VIEW: &str = "Landroid/view/View;";

/// Calls a static `RnViews` method taking the view first.
fn views(name: &str, signature: &str, args: &[Arg<'_>]) -> Result<(), Error> {
    call_static(Class::Views, name, signature, args).map(|_| ())
}

/// Whether `node` is a container that scrolls, and along which axes (1
/// horizontal, 2 vertical, 3 both).
pub(crate) fn scroll_axes(node: &TreeNode) -> Option<i32> {
    let overflow = match node.kind {
        NodeKind::Column => node.column_style.map(|style| style.overflow),
        NodeKind::Row => node.row_style.map(|style| style.overflow),
        _ => None,
    };
    (overflow == Some(Overflow::Scroll)).then_some(match node.kind {
        NodeKind::Row => 1,
        _ => 2,
    })
}

/// Whether `node` is a container that scrolls.
pub(crate) fn scrolls(node: &TreeNode) -> bool {
    scroll_axes(node).is_some()
}

/// The shape a node would be realized in.
pub(crate) fn shape_of(node: &TreeNode) -> Shape {
    if let Some(kind) = &node.foreign {
        return Shape::Foreign(kind.clone());
    }
    match node.kind {
        NodeKind::Column | NodeKind::Row => {
            scroll_axes(node).map_or(Shape::Plain, Shape::Scrolling)
        }
        NodeKind::Control => {
            node.control.as_ref().map_or(Shape::Plain, |control| Shape::Control(variant(control)))
        }
        _ => Shape::Plain,
    }
}

/// Whether `object` can no longer realize `node` and must be replaced.
pub(crate) fn needs_replacement(object: &HostObject, node: &TreeNode) -> bool {
    object.kind != node.kind || object.shape != shape_of(node)
}

/// The name of a control's variant, which decides its view class.
pub(crate) const fn variant(control: &Control) -> &'static str {
    match control {
        Control::Checkbox { .. } => "checkbox",
        Control::Radio { .. } => "radio",
        Control::Toggle { .. } => "toggle",
        Control::Slider { .. } => "slider",
        Control::Progress { .. } => "progress",
        Control::Select { .. } => "select",
        Control::ListBox { .. } => "list-box",
        Control::DatePicker { .. } => "date-picker",
        Control::Spinner { .. } => "spinner",
        Control::Separator => "separator",
        Control::Link { .. } => "link",
        Control::MultilineText { .. } => "multiline-text",
        Control::Image { .. } => "image",
        _ => "unknown",
    }
}

/// The host library's kind for a control.
pub(crate) const fn control_kind(control: &Control) -> i32 {
    match control {
        Control::Checkbox { .. } => protocol::CHECKBOX,
        Control::Radio { .. } => protocol::RADIO,
        Control::Toggle { .. } => protocol::TOGGLE,
        Control::Slider { .. } => protocol::SLIDER,
        Control::Progress { .. } => protocol::PROGRESS,
        Control::Select { .. } => protocol::SELECT,
        Control::ListBox { .. } => protocol::LIST_BOX,
        Control::DatePicker { .. } => protocol::DATE,
        Control::Spinner { .. } => protocol::SPINNER,
        Control::Separator => protocol::SEPARATOR,
        Control::Link { .. } => protocol::LINK,
        Control::MultilineText { .. } => protocol::MULTILINE,
        Control::Image { .. } => protocol::IMAGE,
        _ => protocol::LABEL,
    }
}

/// The host library's kind for `node`.
pub(crate) fn java_kind(node: &TreeNode) -> i32 {
    if node.foreign.is_some() {
        return protocol::FOREIGN;
    }
    match node.kind {
        NodeKind::Label => protocol::LABEL,
        NodeKind::Button => protocol::BUTTON,
        NodeKind::TextInput => protocol::TEXT_INPUT,
        NodeKind::Canvas => protocol::CANVAS,
        NodeKind::Surface => protocol::SURFACE,
        NodeKind::TabBar => protocol::TAB_BAR,
        NodeKind::Column | NodeKind::Row => {
            if scrolls(node) {
                protocol::SCROLL
            } else {
                protocol::CONTAINER
            }
        }
        NodeKind::Control => node.control.as_ref().map_or(protocol::LABEL, control_kind),
    }
}

/// Creates the host object realizing `node` in window `window` of
/// `activity`, reporting with `tag`.
pub(crate) fn create(
    activity: &JavaRef,
    node: &TreeNode,
    window: u64,
    tag: i32,
) -> Result<HostObject, Error> {
    let java_kind = java_kind(node);
    let axes = scroll_axes(node).unwrap_or(2);
    let view = call_static(
        Class::Views,
        "create",
        "(Landroid/app/Activity;IJII)Landroid/view/View;",
        &[
            Arg::Obj(activity),
            Arg::Int(java_kind),
            Arg::Long(i64::try_from(window).unwrap_or(0)),
            Arg::Int(tag),
            Arg::Int(axes),
        ],
    )?
    .obj()
    .ok_or_else(|| Error::java("RnViews.create", "returned no view"))?;
    let content = match java_kind {
        protocol::CONTAINER => Some(view.clone()),
        protocol::SCROLL => call_static(
            Class::Views,
            "scrollContent",
            "(Landroid/view/View;)Ldev/rustnative/android/RnLayout;",
            &[Arg::Obj(&view)],
        )?
        .obj(),
        _ => None,
    };
    let object = HostObject {
        kind: node.kind,
        java_kind,
        view,
        content,
        shape: shape_of(node),
        tag,
        style: None,
        drop: false,
    };
    update(&object, node)?;
    Ok(object)
}

/// Brings `object`'s view in line with `node`'s content (text, value,
/// options), leaving what is already right alone (the Java setters compare
/// before they change anything).
pub(crate) fn update(object: &HostObject, node: &TreeNode) -> Result<(), Error> {
    let view = &object.view;
    match node.kind {
        NodeKind::Label | NodeKind::Button | NodeKind::TextInput => {
            set_text(view, node.text.as_deref().unwrap_or_default())?;
        }
        NodeKind::Canvas => {
            let commands = node.draw_list.as_ref().map(crate::canvas::encode).unwrap_or_default();
            call_static(
                Class::Canvas,
                "setCommands",
                "(Landroid/view/View;[B)V",
                &[Arg::Obj(view), Arg::Bytes(&commands)],
            )?;
        }
        NodeKind::TabBar => {
            if let Some(tabs) = &node.tabs {
                set_options(view, tabs.labels(), i32::try_from(tabs.selected()).unwrap_or(-1))?;
            }
        }
        NodeKind::Control => {
            if let Some(control) = &node.control {
                update_control(view, control)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn update_control(view: &JavaRef, control: &Control) -> Result<(), Error> {
    match control {
        Control::Checkbox { label, checked } => {
            set_text(view, label)?;
            set_checked(view, *checked)
        }
        Control::Radio { label, selected } => {
            set_text(view, label)?;
            set_checked(view, *selected)
        }
        Control::Toggle { label, on } => {
            set_text(view, label)?;
            set_checked(view, *on)
        }
        Control::Slider { value, min, max } | Control::Spinner { value, min, max } => views(
            "setRange",
            &format!("({VIEW}JJJ)V"),
            &[Arg::Obj(view), Arg::Long(*min), Arg::Long(*max), Arg::Long(*value)],
        ),
        Control::Progress { percent } => views(
            "setProgress",
            &format!("({VIEW}I)V"),
            &[Arg::Obj(view), Arg::Int(percent.map_or(-1, i32::from))],
        ),
        Control::Select { options, selected } => set_options(view, options, index(*selected)),
        Control::ListBox { items, selected } => set_options(view, items, index(*selected)),
        Control::DatePicker { date } => views(
            "setDate",
            &format!("({VIEW}IIILjava/lang/String;)V"),
            &[
                Arg::Obj(view),
                Arg::Int(date.year),
                Arg::Int(i32::from(date.month)),
                Arg::Int(i32::from(date.day)),
                Arg::Str(&date.to_string()),
            ],
        ),
        Control::Link { text } => set_text(view, text),
        Control::MultilineText { value } => set_text(view, value),
        Control::Image { image } => {
            let pixels = crate::pixels::premultiplied_rgba(image);
            views(
                "setImage",
                &format!("({VIEW}II[B)V"),
                &[
                    Arg::Obj(view),
                    Arg::Int(i32::try_from(image.width()).unwrap_or(0)),
                    Arg::Int(i32::try_from(image.height()).unwrap_or(0)),
                    Arg::Bytes(&pixels),
                ],
            )
        }
        _ => Ok(()),
    }
}

fn index(selected: Option<usize>) -> i32 {
    selected.and_then(|index| i32::try_from(index).ok()).unwrap_or(-1)
}

pub(crate) fn set_text(view: &JavaRef, text: &str) -> Result<(), Error> {
    views("setText", &format!("({VIEW}Ljava/lang/String;)V"), &[Arg::Obj(view), Arg::Str(text)])
}

pub(crate) fn set_checked(view: &JavaRef, checked: bool) -> Result<(), Error> {
    views("setChecked", &format!("({VIEW}Z)V"), &[Arg::Obj(view), Arg::Bool(checked)])
}

pub(crate) fn set_options(view: &JavaRef, options: &[String], selected: i32) -> Result<(), Error> {
    views(
        "setOptions",
        &format!("({VIEW}[Ljava/lang/String;I)V"),
        &[Arg::Obj(view), Arg::Strs(options), Arg::Int(selected)],
    )
}

pub(crate) fn set_enabled(view: &JavaRef, enabled: bool) -> Result<(), Error> {
    views("setEnabled", &format!("({VIEW}Z)V"), &[Arg::Obj(view), Arg::Bool(enabled)])
}

pub(crate) fn set_visible(view: &JavaRef, visible: bool) -> Result<(), Error> {
    views("setVisible", &format!("({VIEW}Z)V"), &[Arg::Obj(view), Arg::Bool(visible)])
}

pub(crate) fn set_alpha(view: &JavaRef, alpha: f32) -> Result<(), Error> {
    views("setAlpha", &format!("({VIEW}F)V"), &[Arg::Obj(view), Arg::Float(alpha)])
}

pub(crate) fn set_focusable(view: &JavaRef, focusable: bool) -> Result<(), Error> {
    views("setFocusable", &format!("({VIEW}Z)V"), &[Arg::Obj(view), Arg::Bool(focusable)])
}

pub(crate) fn set_direction(view: &JavaRef, right_to_left: bool) -> Result<(), Error> {
    views("setDirection", &format!("({VIEW}Z)V"), &[Arg::Obj(view), Arg::Bool(right_to_left)])
}

/// Places `view` in its container at `(x, y)` with size `width` × `height`,
/// in pixels.
pub(crate) fn place(view: &JavaRef, x: i32, y: i32, width: i32, height: i32) -> Result<(), Error> {
    call_static(
        Class::Layout,
        "place",
        "(Landroid/view/View;IIII)V",
        &[Arg::Obj(view), Arg::Int(x), Arg::Int(y), Arg::Int(width), Arg::Int(height)],
    )
    .map(|_| ())
}

/// Puts `child` into `parent` at `index` (-1: the end); a no-op when it is
/// already there.
pub(crate) fn insert(parent: &JavaRef, child: &JavaRef, index: i32) -> Result<(), Error> {
    call_static(
        Class::Layout,
        "insert",
        "(Landroid/view/ViewGroup;Landroid/view/View;I)V",
        &[Arg::Obj(parent), Arg::Obj(child), Arg::Int(index)],
    )
    .map(|_| ())
}

/// Sets the size a scroll container's content layout scrolls over, in
/// pixels.
pub(crate) fn set_content_size(content: &JavaRef, width: i32, height: i32) -> Result<(), Error> {
    call_static(
        Class::Layout,
        "setContentSize",
        "(Ldev/rustnative/android/RnLayout;II)V",
        &[Arg::Obj(content), Arg::Int(width), Arg::Int(height)],
    )
    .map(|_| ())
}

/// Where a scroll container is scrolled to, in pixels.
pub(crate) fn scroll_offset(view: &JavaRef) -> (i32, i32) {
    let values =
        call_static(Class::Views, "scrollOffset", "(Landroid/view/View;)[I", &[Arg::Obj(view)])
            .map(crate::jni_host::Ret::ints)
            .unwrap_or_default();
    (values.first().copied().unwrap_or(0), values.get(1).copied().unwrap_or(0))
}

/// Scrolls a scroll container to `(x, y)` pixels.
pub(crate) fn scroll_to(view: &JavaRef, x: i32, y: i32) -> Result<(), Error> {
    views("scrollTo", &format!("({VIEW}II)V"), &[Arg::Obj(view), Arg::Int(x), Arg::Int(y)])
}

/// A view's frame in its parent: left, top, width, height pixels.
pub(crate) fn frame(view: &JavaRef) -> [i32; 4] {
    let values = call_static(Class::Views, "frame", "(Landroid/view/View;)[I", &[Arg::Obj(view)])
        .map(crate::jni_host::Ret::ints)
        .unwrap_or_default();
    let mut out = [0; 4];
    for (slot, value) in out.iter_mut().zip(values) {
        *slot = value;
    }
    out
}

#[cfg(feature = "device-tests")]
/// A view's Java class name.
pub(crate) fn class_name(view: &JavaRef) -> String {
    call_static(
        Class::Views,
        "className",
        "(Landroid/view/View;)Ljava/lang/String;",
        &[Arg::Obj(view)],
    )
    .ok()
    .and_then(crate::jni_host::Ret::string)
    .unwrap_or_default()
}

#[cfg(feature = "device-tests")]
/// A view's text, if it shows text.
pub(crate) fn text(view: &JavaRef) -> Option<String> {
    call_static(Class::Views, "text", "(Landroid/view/View;)Ljava/lang/String;", &[Arg::Obj(view)])
        .ok()
        .and_then(crate::jni_host::Ret::string)
}
