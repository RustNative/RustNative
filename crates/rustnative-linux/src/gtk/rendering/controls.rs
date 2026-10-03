//! Creating, updating, and wiring the widget that realizes each node kind
//! (`docs/linux/widget-mapping.md`).
//!
//! | Node | GTK widget |
//! |---|---|
//! | Column, Row | `RnLayout`; inside a `GtkScrolledWindow` when it scrolls |
//! | Label | `GtkLabel` (wrapping, not selectable) |
//! | Button | `GtkButton` |
//! | `TextInput` | `GtkEntry` |
//! | `TabBar` | a tab-list `RnLayout` of grouped `GtkToggleButton`s, each a tab |
//! | Checkbox, Radio | `GtkCheckButton` (a radio joins its siblings' group) |
//! | Toggle | `GtkSwitch` beside its `GtkLabel`, labelled by it |
//! | Slider | `GtkScale` |
//! | Progress | `GtkProgressBar` (pulsing while unknown) |
//! | Select | `GtkDropDown` over a `GtkStringList` |
//! | `ListBox` | `GtkListBox` of `GtkLabel`s |
//! | `DatePicker` | `GtkMenuButton` showing the date, with a `GtkCalendar` popover |
//! | Spinner | `GtkSpinButton` |
//! | Separator | `GtkSeparator` |
//! | Link | a `GtkButton` in the link style, with the link role |
//! | `MultilineText` | `GtkTextView` in a `GtkScrolledWindow` |
//! | Image | `GtkPicture` over a `GdkMemoryTexture` |
//! | Canvas | `RnCanvas` (`gtk::canvas`) |
//! | Surface | `RnLayout` holding the native surface (`gtk::surface`) |
//!
//! Every widget that reports the person's changes has its handlers
//! registered on its [`HostObject`], and every framework change is made
//! through [`HostObject::quietly`], so a component hears about what the
//! person did and never about what it rendered.

use gtk::glib;
use gtk::prelude::*;
use rustnative_core::{
    AccessibleAction, AccessibleActionKind, AccessibleValue, CalendarDate, Control, Event, NodeId,
    NodeKind, Overflow, Scalar, TreeNode, WindowId,
};

use super::{HostObject, Shape};
use crate::gtk::accessibility::construct_role;
use crate::gtk::backend::{Work, post, post_later};
use crate::gtk::layout_widget::{RnControl, RnLayout};

/// Reports `event` for `window` through the backend queue.
fn report(window: WindowId, event: Event) {
    post(Work::Event(window, event));
}

/// Whether `node` is a container that scrolls.
pub(crate) fn scrolls(node: &TreeNode) -> bool {
    let overflow = match node.kind {
        NodeKind::Column => node.column_style.map(|style| style.overflow),
        NodeKind::Row => node.row_style.map(|style| style.overflow),
        _ => None,
    };
    overflow == Some(Overflow::Scroll)
}

/// The shape a node would be realized in.
pub(crate) fn shape_of(node: &TreeNode) -> Shape {
    if let Some(kind) = &node.foreign {
        return Shape::Foreign(kind.clone());
    }
    match node.kind {
        NodeKind::Column | NodeKind::Row if scrolls(node) => Shape::Scrolling,
        NodeKind::Column | NodeKind::Row if interactive(node) => Shape::Interactive,
        NodeKind::Control => {
            node.control.as_ref().map_or(Shape::Plain, |control| Shape::Control(variant(control)))
        }
        NodeKind::TabBar => Shape::Tabs(node.tabs.as_ref().map_or(0, |tabs| tabs.labels().len())),
        _ => Shape::Plain,
    }
}

/// Whether `node` is a container assistive technology can operate: it
/// declared an Invoke action or a range value.
pub(crate) fn interactive(node: &TreeNode) -> bool {
    node.accessibility.supports(AccessibleActionKind::Invoke)
        || matches!(node.accessibility.value(), Some(AccessibleValue::Range { .. }))
}

/// Whether `object` can no longer realize `node` and must be replaced: its
/// kind, its construct-only role, or its shape changed.
pub(crate) fn needs_replacement(object: &HostObject, node: &TreeNode) -> bool {
    object.kind != node.kind
        || object.role != construct_role(node)
        || object.shape != shape_of(node)
}

/// The name of a control's variant, which decides its widget type.
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

/// Creates the host object realizing `node` in `window`.
pub(crate) fn create(node: &TreeNode, window: WindowId) -> Result<HostObject, crate::Error> {
    let role = construct_role(node);
    let id = node.id;
    let shape = shape_of(node);
    let mut signals = Vec::new();
    let (widget, content): (gtk::Widget, Option<RnLayout>) = match node.kind {
        NodeKind::Column | NodeKind::Row => {
            let content = RnLayout::with_role(if scrolls(node) {
                gtk::AccessibleRole::Generic
            } else {
                role
            });
            if scrolls(node) {
                let scrolled: gtk::ScrolledWindow = glib::Object::builder()
                    .property("accessible-role", role)
                    .property("hscrollbar-policy", gtk::PolicyType::Automatic)
                    .property("vscrollbar-policy", gtk::PolicyType::Automatic)
                    .build();
                scrolled.set_child(Some(&content));
                (scrolled.upcast(), Some(content))
            } else if interactive(node) {
                let control = RnControl::with_role(role);
                control.connect_activated(move || {
                    report(
                        window,
                        Event::AccessibilityAction {
                            target: id,
                            element: None,
                            action: AccessibleAction::Invoke,
                        },
                    );
                });
                control.connect_value_requested(move |value| {
                    #[allow(clippy::cast_possible_truncation, reason = "an accessible range value")]
                    let value = Scalar::new(value as f32);
                    report(
                        window,
                        Event::AccessibilityAction {
                            target: id,
                            element: None,
                            action: AccessibleAction::SetRangeValue(value),
                        },
                    );
                });
                control.set_overflow(gtk::Overflow::Hidden);
                let content: RnLayout = control.clone().upcast();
                (control.upcast(), Some(content))
            } else {
                content.set_overflow(gtk::Overflow::Hidden);
                (content.clone().upcast(), Some(content))
            }
        }
        NodeKind::Label => {
            let label: gtk::Label = glib::Object::builder()
                .property("accessible-role", role)
                .property("wrap", true)
                .property("wrap-mode", gtk::pango::WrapMode::WordChar)
                .property("xalign", 0.0_f32)
                .property("yalign", 0.0_f32)
                .build();
            label.set_text(node.text.as_deref().unwrap_or_default());
            (label.upcast(), None)
        }
        NodeKind::Button => {
            let button: gtk::Button =
                glib::Object::builder().property("accessible-role", role).build();
            button.set_label(node.text.as_deref().unwrap_or_default());
            let handler =
                button.connect_clicked(move |_| report(window, Event::Click { target: id }));
            signals.push((button.clone().upcast(), handler));
            (button.upcast(), None)
        }
        NodeKind::TextInput => {
            let entry: gtk::Entry =
                glib::Object::builder().property("accessible-role", role).build();
            entry.set_text(node.text.as_deref().unwrap_or_default());
            let handler = entry.connect_changed(move |entry| {
                report(window, Event::TextChanged { target: id, value: entry.text().to_string() });
            });
            signals.push((entry.clone().upcast(), handler));
            (entry.upcast(), None)
        }
        NodeKind::TabBar => {
            let bar = RnLayout::with_role(gtk::AccessibleRole::TabList);
            bar.add_css_class("linked");
            let labels = node.tabs.as_ref().map(|tabs| tabs.labels().to_vec()).unwrap_or_default();
            let mut first: Option<gtk::ToggleButton> = None;
            for (index, label) in labels.iter().enumerate() {
                let tab: gtk::ToggleButton = glib::Object::builder()
                    .property("accessible-role", gtk::AccessibleRole::Tab)
                    .build();
                tab.set_label(label);
                if let Some(first) = &first {
                    tab.set_group(Some(first));
                } else {
                    first = Some(tab.clone());
                }
                let handler = tab.connect_toggled(move |tab| {
                    if tab.is_active() {
                        report(window, Event::TabSelected { target: id, index });
                    }
                });
                signals.push((tab.clone().upcast(), handler));
                bar.append(&tab);
            }
            (bar.clone().upcast(), None)
        }
        NodeKind::Control => {
            let control = node.control.clone().unwrap_or(Control::Separator);
            let widget = create_control(&control, id, window, &mut signals);
            (widget, None)
        }
        NodeKind::Canvas => {
            let canvas = crate::gtk::canvas::RnCanvas::create(role);
            canvas.set_overflow(gtk::Overflow::Hidden);
            // Its container half carries the canvas' virtual elements.
            let content: RnLayout = canvas.clone().upcast();
            (canvas.upcast(), Some(content))
        }
        NodeKind::Surface if node.foreign.is_some() => {
            // A foreign widget (`gtk::foreign`): an application's, or host
            // content. Its accessibility and name are its own.
            let kind = node.foreign.clone().unwrap_or_default();
            let foreign = crate::gtk::foreign::create(&kind, id)?;
            return Ok(HostObject {
                kind: node.kind,
                widget: foreign.widget,
                content: None,
                role,
                shape,
                signals,
                drop: None,
            });
        }
        NodeKind::Surface => {
            let host = RnLayout::with_role(role);
            {
                // A rendered surface follows its host's every allocation
                // (`gtk::surface`), after GTK's layout phase.
                host.connect_allocated(move |_, _| post_later(Work::SurfaceAllocated(window, id)));
            }
            (host.upcast(), None)
        }
    };
    widget.set_widget_name(&widget_name(node.id));
    let object = HostObject { kind: node.kind, widget, content, role, shape, signals, drop: None };
    update(&object, node);
    Ok(object)
}

/// The widget name every realized widget carries: its node's key, so a
/// person reading GTK's inspector (or an AT-SPI tree) can tell which node
/// it is.
pub(crate) fn widget_name(id: NodeId) -> String {
    format!("rn:{}", id.local_key().unwrap_or_else(|| format!("{id:?}")))
}

fn create_control(
    control: &Control,
    id: NodeId,
    window: WindowId,
    signals: &mut Vec<(glib::Object, glib::SignalHandlerId)>,
) -> gtk::Widget {
    match control {
        Control::Checkbox { .. } | Control::Radio { .. } => {
            let radio = matches!(control, Control::Radio { .. });
            let check = gtk::CheckButton::new();
            let handler = check.connect_toggled(move |check| {
                // Radios share a GTK group (`realization::group_radios`) so
                // they look and read as radios; choosing one also turns its
                // sibling off, which is GTK's bookkeeping, not the person's
                // choice, and is not reported.
                if !radio || check.is_active() {
                    report(window, Event::Toggled { target: id, on: check.is_active() });
                }
            });
            signals.push((check.clone().upcast(), handler));
            check.upcast()
        }
        Control::Toggle { .. } => {
            let row = RnLayout::with_role(gtk::AccessibleRole::Generic);
            let switch = gtk::Switch::new();
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            switch.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
            row.append(&switch);
            row.append(&label);
            let handler = switch.connect_active_notify(move |switch| {
                report(window, Event::Toggled { target: id, on: switch.is_active() });
            });
            signals.push((switch.clone().upcast(), handler));
            row.upcast()
        }
        Control::Slider { .. } => {
            let scale = gtk::Scale::new(gtk::Orientation::Horizontal, None::<&gtk::Adjustment>);
            scale.set_draw_value(false);
            let handler = scale.connect_value_changed(move |scale| {
                report(window, Event::ValueChanged { target: id, value: rounded(scale.value()) });
            });
            signals.push((scale.clone().upcast(), handler));
            scale.upcast()
        }
        Control::Spinner { .. } => {
            let spin = gtk::SpinButton::new(None::<&gtk::Adjustment>, 1.0, 0);
            let handler = spin.connect_value_changed(move |spin| {
                report(window, Event::ValueChanged { target: id, value: rounded(spin.value()) });
            });
            signals.push((spin.clone().upcast(), handler));
            spin.upcast()
        }
        Control::Progress { .. } => gtk::ProgressBar::new().upcast(),
        Control::Select { .. } => {
            let dropdown =
                gtk::DropDown::new(Some(gtk::StringList::new(&[])), None::<gtk::Expression>);
            let handler = dropdown.connect_selected_notify(move |dropdown| {
                report(
                    window,
                    Event::SelectionChanged { target: id, index: index_of(dropdown.selected()) },
                );
            });
            signals.push((dropdown.clone().upcast(), handler));
            dropdown.upcast()
        }
        Control::ListBox { .. } => {
            let list = gtk::ListBox::new();
            list.set_selection_mode(gtk::SelectionMode::Single);
            let handler = list.connect_row_selected(move |_, row| {
                let index = row.and_then(|row| usize::try_from(row.index()).ok());
                report(window, Event::SelectionChanged { target: id, index });
            });
            signals.push((list.clone().upcast(), handler));
            list.upcast()
        }
        Control::DatePicker { .. } => {
            let calendar = gtk::Calendar::new();
            let popover = gtk::Popover::new();
            popover.set_child(Some(&calendar));
            let button = gtk::MenuButton::new();
            button.set_popover(Some(&popover));
            let handler = calendar.connect_day_selected(move |calendar| {
                if let Some(date) = calendar_date(&calendar.date()) {
                    report(window, Event::DateChanged { target: id, date });
                }
            });
            signals.push((calendar.upcast(), handler));
            button.upcast()
        }
        Control::Link { .. } => {
            let link: gtk::Button = glib::Object::builder()
                .property("accessible-role", gtk::AccessibleRole::Link)
                .build();
            link.add_css_class("link");
            link.set_has_frame(false);
            let handler =
                link.connect_clicked(move |_| report(window, Event::Click { target: id }));
            signals.push((link.clone().upcast(), handler));
            link.upcast()
        }
        Control::MultilineText { .. } => {
            let view = gtk::TextView::new();
            view.set_wrap_mode(gtk::WrapMode::WordChar);
            let buffer = view.buffer();
            // A replacement (a paste over a selection, `set_text`) is a
            // deletion and an insertion, each emitting `changed`; the person
            // made one change, so one is reported, once the buffer settles.
            let pending = std::rc::Rc::new(std::cell::Cell::new(false));
            let handler = buffer.connect_changed(move |buffer| {
                if pending.replace(true) {
                    return;
                }
                let buffer = buffer.downgrade();
                let pending = std::rc::Rc::clone(&pending);
                glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
                    pending.set(false);
                    if let Some(buffer) = buffer.upgrade() {
                        let value = buffer
                            .text(&buffer.start_iter(), &buffer.end_iter(), false)
                            .to_string();
                        report(window, Event::TextChanged { target: id, value });
                    }
                    glib::ControlFlow::Break
                });
            });
            signals.push((buffer.upcast(), handler));
            let scrolled = gtk::ScrolledWindow::new();
            scrolled.set_child(Some(&view));
            scrolled.set_has_frame(true);
            scrolled.upcast()
        }
        Control::Image { .. } => {
            let picture = gtk::Picture::new();
            picture.set_can_shrink(true);
            picture.upcast()
        }
        _ => gtk::Separator::new(gtk::Orientation::Horizontal).upcast(),
    }
}

/// A widget of the kind that realizes `control`, configured with its
/// content — what the measurer measures.
pub(crate) fn prototype(control: &Control) -> gtk::Widget {
    let mut signals = Vec::new();
    let widget =
        create_control(control, NodeId::from_key("rn:prototype"), WindowId::PRIMARY, &mut signals);
    // A prototype reports nothing: its handlers are removed, not blocked.
    for (object, handler) in signals {
        object.disconnect(handler);
    }
    update_control(&widget, control);
    widget
}

/// Applies `node`'s content (text, value, state) to `object`, quietly.
pub(crate) fn update(object: &HostObject, node: &TreeNode) {
    object.quietly(|| match node.kind {
        NodeKind::Label => {
            if let Some(label) = object.widget.downcast_ref::<gtk::Label>() {
                let text = node.text.as_deref().unwrap_or_default();
                if label.text() != text {
                    label.set_text(text);
                }
            }
        }
        NodeKind::Button => {
            if let Some(button) = object.widget.downcast_ref::<gtk::Button>() {
                let text = node.text.as_deref().unwrap_or_default();
                if button.label().as_deref() != Some(text) {
                    button.set_label(text);
                }
            }
        }
        NodeKind::TextInput => {
            if let Some(entry) = object.widget.downcast_ref::<gtk::Entry>() {
                let text = node.text.as_deref().unwrap_or_default();
                // Setting an unchanged value would move the caret to the
                // end mid-typing.
                if entry.text() != text {
                    entry.set_text(text);
                }
            }
        }
        NodeKind::TabBar => {
            if let (Some(tabs), Some(bar)) = (&node.tabs, object.widget.downcast_ref::<RnLayout>())
            {
                let mut child = bar.first_child();
                let mut index = 0;
                while let Some(widget) = child {
                    child = widget.next_sibling();
                    if let Some(tab) = widget.downcast_ref::<gtk::ToggleButton>() {
                        if let Some(label) = tabs.labels().get(index) {
                            if tab.label().as_deref() != Some(label.as_str()) {
                                tab.set_label(label);
                            }
                        }
                        let selected = index == tabs.selected();
                        if tab.is_active() != selected {
                            tab.set_active(selected);
                        }
                        tab.update_state(&[gtk::accessible::State::Selected(Some(selected))]);
                        index += 1;
                    }
                }
            }
        }
        NodeKind::Control => {
            if let Some(control) = &node.control {
                update_control(&object.widget, control);
            }
        }
        NodeKind::Canvas => {
            if let (Some(canvas), Some(list)) =
                (object.widget.downcast_ref::<crate::gtk::canvas::RnCanvas>(), &node.draw_list)
            {
                canvas.set_draw_list(list);
            }
        }
        NodeKind::Column | NodeKind::Row | NodeKind::Surface => {}
    });
}

fn update_control(widget: &gtk::Widget, control: &Control) {
    match control {
        Control::Checkbox { label, checked } => {
            if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                set_label(check, label);
                if check.is_active() != *checked {
                    check.set_active(*checked);
                }
            }
        }
        Control::Radio { label, selected } => {
            if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                set_label(check, label);
                if check.is_active() != *selected {
                    check.set_active(*selected);
                }
            }
        }
        Control::Toggle { label, on } => {
            if let Some(row) = widget.downcast_ref::<RnLayout>() {
                let switch =
                    row.first_child().and_then(|child| child.downcast::<gtk::Switch>().ok());
                let text = switch
                    .as_ref()
                    .and_then(WidgetExt::next_sibling)
                    .and_then(|child| child.downcast::<gtk::Label>().ok());
                if let Some(text) = text {
                    if text.text() != *label {
                        text.set_text(label);
                    }
                }
                if let Some(switch) = switch {
                    if switch.is_active() != *on {
                        switch.set_active(*on);
                    }
                }
                place_toggle(row);
            }
        }
        Control::Slider { value, min, max } => {
            if let Some(scale) = widget.downcast_ref::<gtk::Scale>() {
                set_range(&scale.adjustment(), *value, *min, *max);
            }
        }
        Control::Spinner { value, min, max } => {
            if let Some(spin) = widget.downcast_ref::<gtk::SpinButton>() {
                set_range(&spin.adjustment(), *value, *min, *max);
            }
        }
        Control::Progress { percent } => {
            if let Some(bar) = widget.downcast_ref::<gtk::ProgressBar>() {
                match percent {
                    Some(percent) => bar.set_fraction(f64::from((*percent).min(100)) / 100.0),
                    None => bar.pulse(),
                }
            }
        }
        Control::Select { options, selected } => {
            if let Some(dropdown) = widget.downcast_ref::<gtk::DropDown>() {
                let model = dropdown.model().and_downcast::<gtk::StringList>();
                if let Some(model) = model {
                    let current: Vec<String> = (0..model.n_items())
                        .filter_map(|index| model.string(index).map(|s| s.to_string()))
                        .collect();
                    if current != *options {
                        let replacement: Vec<&str> = options.iter().map(String::as_str).collect();
                        model.splice(0, model.n_items(), &replacement);
                    }
                }
                let wanted = selected
                    .and_then(|index| u32::try_from(index).ok())
                    .unwrap_or(gtk::INVALID_LIST_POSITION);
                if dropdown.selected() != wanted {
                    dropdown.set_selected(wanted);
                }
            }
        }
        Control::ListBox { items, selected } => {
            if let Some(list) = widget.downcast_ref::<gtk::ListBox>() {
                let mut rows = Vec::new();
                let mut index = 0;
                while let Some(row) = list.row_at_index(index) {
                    rows.push(row);
                    index += 1;
                }
                let texts: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        row.child()
                            .and_downcast::<gtk::Label>()
                            .map(|label| label.text().to_string())
                            .unwrap_or_default()
                    })
                    .collect();
                if texts != *items {
                    list.remove_all();
                    for item in items {
                        let label = gtk::Label::new(Some(item));
                        label.set_xalign(0.0);
                        list.append(&label);
                    }
                }
                let wanted = selected
                    .and_then(|index| i32::try_from(index).ok())
                    .and_then(|index| list.row_at_index(index));
                if list.selected_row() != wanted {
                    list.select_row(wanted.as_ref());
                }
            }
        }
        Control::DatePicker { date } => {
            if let Some(button) = widget.downcast_ref::<gtk::MenuButton>() {
                button.set_label(&format!("{:04}-{:02}-{:02}", date.year, date.month, date.day));
                let calendar = button
                    .popover()
                    .and_then(|popover| popover.child())
                    .and_downcast::<gtk::Calendar>();
                if let Some(calendar) = calendar {
                    if calendar_date(&calendar.date()) != Some(*date) {
                        if let Ok(value) = glib::DateTime::from_local(
                            date.year,
                            i32::from(date.month),
                            i32::from(date.day),
                            0,
                            0,
                            0.0,
                        ) {
                            calendar.select_day(&value);
                        }
                    }
                }
            }
        }
        Control::Link { text } => {
            if let Some(link) = widget.downcast_ref::<gtk::Button>() {
                if link.label().as_deref() != Some(text.as_str()) {
                    link.set_label(text);
                }
            }
        }
        Control::MultilineText { value } => {
            let view = widget
                .downcast_ref::<gtk::ScrolledWindow>()
                .and_then(gtk::ScrolledWindow::child)
                .and_downcast::<gtk::TextView>();
            if let Some(view) = view {
                let buffer = view.buffer();
                let current = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                if current != *value {
                    buffer.set_text(value);
                }
            }
        }
        Control::Image { image } => {
            if let Some(picture) = widget.downcast_ref::<gtk::Picture>() {
                picture.set_paintable(texture(image).as_ref());
            }
        }
        _ => {}
    }
}

/// Lays a toggle's switch and label side by side inside its row: the one
/// container here GTK sizes, because its contents are one control.
fn place_toggle(row: &RnLayout) {
    let Some(switch) = row.first_child() else { return };
    let (_, switch_width, _, _) = switch.measure(gtk::Orientation::Horizontal, -1);
    let (_, switch_height, _, _) = switch.measure(gtk::Orientation::Vertical, -1);
    row.place(&switch, rustnative_core::Rect::new(0, 0, switch_width, switch_height));
    if let Some(label) = switch.next_sibling() {
        let (_, width, _, _) = label.measure(gtk::Orientation::Horizontal, -1);
        let (_, height, _, _) = label.measure(gtk::Orientation::Vertical, -1);
        let y = (switch_height - height).max(0) / 2;
        row.place(&label, rustnative_core::Rect::new(switch_width + 8, y, width, height));
        row.set_content_size(Some((switch_width + 8 + width, switch_height.max(height))));
    }
}

fn set_label(check: &gtk::CheckButton, label: &str) {
    if check.label().as_deref() != Some(label) {
        check.set_label(Some(label));
    }
}

#[allow(clippy::cast_precision_loss, reason = "control values are small integers")]
fn set_range(adjustment: &gtk::Adjustment, value: i64, min: i64, max: i64) {
    let (min, max) = (min.min(max) as f64, max.max(min) as f64);
    if (adjustment.lower() - min).abs() > f64::EPSILON
        || (adjustment.upper() - max).abs() > f64::EPSILON
    {
        adjustment.configure((value as f64).clamp(min, max), min, max, 1.0, 10.0, 0.0);
    } else if (adjustment.value() - value as f64).abs() > f64::EPSILON {
        adjustment.set_value(value as f64);
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "a slider or spinner value, rounded, inside the i64 range its min and max came from"
)]
fn rounded(value: f64) -> i64 {
    value.round() as i64
}

fn index_of(position: u32) -> Option<usize> {
    (position != gtk::INVALID_LIST_POSITION).then(|| usize::try_from(position).ok()).flatten()
}

fn calendar_date(date: &glib::DateTime) -> Option<CalendarDate> {
    CalendarDate::new(
        date.year(),
        u8::try_from(date.month()).ok()?,
        u8::try_from(date.day_of_month()).ok()?,
    )
}

/// A texture of `image`'s pixels.
pub(crate) fn texture(image: &rustnative_core::ImageData) -> Option<gtk::gdk::Texture> {
    let width = i32::try_from(image.width()).ok()?;
    let height = i32::try_from(image.height()).ok()?;
    let stride = usize::try_from(image.width()).ok()?.checked_mul(4)?;
    let format = if image.is_premultiplied() {
        gtk::gdk::MemoryFormat::R8g8b8a8Premultiplied
    } else {
        gtk::gdk::MemoryFormat::R8g8b8a8
    };
    let bytes = glib::Bytes::from(image.pixels());
    Some(gtk::gdk::MemoryTexture::new(width, height, format, &bytes, stride).upcast())
}
