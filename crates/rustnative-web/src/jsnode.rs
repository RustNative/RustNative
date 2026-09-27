//! The runtime's form of a node: what generated client views build in the
//! browser, and what the runtime's `rn.realize` turns into [`Element`]s
//! exactly as [`crate::dom::Realizer`] does.
//!
//! [`from_node`] converts a core [`Node`] into it. Generated code never needs
//! this (it builds the form directly); the equivalence tests do, to hand the
//! same node to both realizers and compare what they produce.
//!
//! [`Element`]: crate::dom::Element

use rustnative_core::accessibility::{CheckedState, LiveRegion};
use rustnative_core::wire::key_of;
use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, AccessibleValue, Node, NodeId, StateStyles,
    SurfaceContent, VisualStyle,
};
use serde_json::{Value, json};

fn role_name(role: AccessibilityRole) -> (String, Option<u8>) {
    match role {
        AccessibilityRole::Heading { level } => ("Heading".to_owned(), Some(level)),
        other => (format!("{other:?}"), None),
    }
}

/// An accessibility description in the runtime's form.
#[must_use]
pub fn accessibility(info: &AccessibilityInfo) -> Value {
    let (role, level) = role_name(info.role());
    let key = |id: NodeId| key_of(id);
    json!({
        "role": role,
        "level": level,
        "name": info.name_hint(),
        "desc": info.description_hint(),
        "auto": info.automation_id_hint(),
        "focusable": info.is_focusable(),
        "value": match info.value() {
            Some(AccessibleValue::Range { min, max, current, .. }) => json!({ "range": [min.get(), max.get(), current.get()] }),
            Some(AccessibleValue::Text(text)) => json!({ "text": text }),
            None => Value::Null,
        },
        "checked": info.checked_state().map(|state| match state {
            CheckedState::Checked => "true",
            CheckedState::Unchecked => "false",
            CheckedState::Mixed => "mixed",
        }),
        "expanded": info.expanded_state(),
        "selected": info.selected_state(),
        "readonly": info.is_read_only(),
        "required": info.is_required(),
        "busy": info.is_busy(),
        "live": match info.live_region() {
            LiveRegion::Polite => Some("polite"),
            LiveRegion::Assertive => Some("assertive"),
            LiveRegion::Off => None,
        },
        "position": info.position(),
        "labelledby": info.labelled_by_node().map(key),
        "describedby": info.described_by_nodes().iter().map(|id| key(*id)).collect::<Vec<_>>(),
        "controls": info.controls_nodes().iter().map(|id| key(*id)).collect::<Vec<_>>(),
    })
}

/// A typed visual style in the runtime's form, or `null` when it sets
/// nothing.
#[must_use]
pub fn visual(style: &VisualStyle) -> Value {
    if style == &VisualStyle::default() {
        return Value::Null;
    }
    let hex = |color| rustnative_style::model::hex(color);
    json!({
        "fg": style.foreground_override().map(hex),
        "bg": style.background_override().map(hex),
        "border": style.border_override().map(hex),
        "radius": style.border_radius_override(),
        "font": style.typography_override().map(|font| json!({ "family": font.family, "size": font.size, "weight": font.weight })),
        "padding": style.padding_override(),
        "shadow": style.shadow_override().map(|layers| {
            rustnative_core::style::decl::StyleValue::Shadow(layers.to_vec().into()).to_string()
        }),
    })
}

fn states(states: &StateStyles) -> Value {
    use rustnative_core::ControlState;
    let get = |state| states.get(state).map_or(Value::Null, visual);
    json!({
        "hover": get(ControlState::Hovered),
        "focus": get(ControlState::Focused),
        "active": get(ControlState::Pressed),
        "disabled": get(ControlState::Disabled),
    })
}

/// `node` and its subtree in the runtime's form.
#[must_use]
pub fn from_node(node: &Node) -> Value {
    let mut out = json!({
        "k": key_of(node.id()),
        "layout": serde_json::to_value(node.layout()).unwrap_or_default(),
        "a11y": accessibility(node.accessibility()),
        "style": visual(node.visual_style()),
        "states": states(node.state_styles()),
        "opacity": node.opacity(),
        "cursor": node.cursor().map(|cursor| format!("{cursor:?}")),
        "hidden": node.is_hidden(),
        "disabled": node.is_disabled(),
        "command": node.command().map(|command| command.name().to_owned()),
        "shared": node.shared_id().map(key_of),
        "index": node.item_index(),
        "sets": node.declarations().iter().map(|set| rustnative_style::web::set_descriptor(*set)).collect::<Vec<_>>(),
    });
    let fields: Value = match node {
        Node::Label(label) => json!({ "kind": "label", "text": label.text() }),
        Node::Button(button) => json!({ "kind": "button", "text": button.text() }),
        Node::TextInput(input) => json!({ "kind": "input", "value": input.value() }),
        Node::TabBar(bar) => {
            json!({ "kind": "tabs", "labels": bar.tabs().labels(), "selected": bar.tabs().selected() })
        }
        Node::Control(_) => {
            json!({ "kind": "control", "control": serde_json::to_value(node.control_state()).unwrap_or_default() })
        }
        Node::Canvas(canvas) => {
            json!({ "kind": "canvas", "svg": serde_json::to_value(crate::svg::draw_list(canvas.draw_list())).unwrap_or_default() })
        }
        Node::Surface(surface) => json!({
            "kind": "surface",
            "foreign": if let SurfaceContent::Foreign(kind) = surface.content() { Some(kind.clone()) } else { None },
        }),
        Node::Column(column) => {
            let children: Vec<Value> = column.children().iter().map(from_node).collect();
            match column.grid() {
                Some(grid) => {
                    json!({ "kind": "grid", "grid": serde_json::to_value(grid).unwrap_or_default(), "children": children })
                }
                None => json!({
                    "kind": "column",
                    "col": serde_json::to_value(node.column_style().unwrap_or_default()).unwrap_or_default(),
                    "virt": node.virtualization().map(|list| serde_json::to_value(list).unwrap_or_default()),
                    "children": children,
                }),
            }
        }
        Node::Row(row) => json!({
            "kind": "row",
            "col": serde_json::to_value(node.row_style().unwrap_or_default()).unwrap_or_default(),
            "virt": node.virtualization().map(|list| serde_json::to_value(list).unwrap_or_default()),
            "children": row.children().iter().map(from_node).collect::<Vec<_>>(),
        }),
    };
    if let (Value::Object(out), Value::Object(fields)) = (&mut out, fields) {
        out.extend(fields);
    }
    out
}
