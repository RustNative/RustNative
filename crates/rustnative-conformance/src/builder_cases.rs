//! The builder spelling of every syntax-equivalence case.

use rustnative_core::{ColumnStyle, Component, ComponentContext, LayoutStyle, Node, RowStyle};

use crate::syntax::{self, Card, CardProps, values};

/// Every case, by name.
#[must_use]
pub fn cases() -> Vec<(&'static str, Node)> {
    vec![
        ("label", Node::label("greeting", "Hello")),
        (
            "label_layout",
            Node::label_with_layout(
                "sized",
                "Sized",
                LayoutStyle::default()
                    .width(values::WIDTH)
                    .height(values::HEIGHT)
                    .margin(values::MARGIN)
                    .align_self(values::CENTER)
                    .constraints(syntax::constraints())
                    .direction(values::RTL),
            ),
        ),
        ("button", Node::button("save", "Save")),
        ("text_input", Node::text_input("name", "Ada")),
        ("canvas", Node::canvas("swatch", syntax::drawing(), LayoutStyle::default())),
        ("surface", Node::native_surface("scene", LayoutStyle::default())),
        ("foreign", Node::foreign("date", "month-calendar", LayoutStyle::default())),
        ("tab_bar", Node::tab_bar("tabs", ["One", "Two"], 1, LayoutStyle::default())),
        ("checkbox", Node::checkbox("remember", "Remember me", true)),
        ("radio", Node::radio("small", "Small", false)),
        ("toggle", Node::toggle("wifi", "Wi-Fi", true)),
        ("slider", Node::slider("volume", 40, 0, 100)),
        ("progress", Node::progress("upload", Some(40))),
        ("select", Node::select("size", ["S", "M"], Some(1))),
        ("list_box", Node::list_box("fruit", ["Apple", "Pear"], None)),
        (
            "date_picker",
            Node::date_picker(
                "due",
                rustnative_core::CalendarDate { year: 2026, month: 9, day: 25 },
            ),
        ),
        ("spinner", Node::spinner("copies", 2, 1, 9)),
        ("separator", Node::separator("rule")),
        ("link", Node::link("help", "Help")),
        ("multiline_text", Node::multiline_text("notes", "Hello")),
        ("image", Node::image("logo", syntax::picture())),
        (
            "column",
            Node::column_with_layout(
                "stack",
                [Node::label("a", "A"), Node::label("b", "B")],
                LayoutStyle::default(),
                ColumnStyle::default()
                    .padding(values::PADDING)
                    .gap(4)
                    .align_items(values::CENTER)
                    .overflow(values::SCROLL),
            ),
        ),
        (
            "row",
            Node::row_with_layout(
                "line",
                [Node::button("ok", "OK"), Node::button("cancel", "Cancel")],
                LayoutStyle::default(),
                RowStyle::default().gap(2),
            ),
        ),
        (
            "virtual_list",
            Node::virtual_list_with_layout(
                "rows",
                syntax::list(),
                LayoutStyle::default(),
                (0..3).map(|index| {
                    Node::label(format!("row-{index}"), format!("Row {index}"))
                        .with_item_index(index)
                }),
            ),
        ),
        (
            "accessibility",
            Node::button("icon", "\u{1F4BE}").with_accessibility(syntax::accessibility()),
        ),
        ("style", Node::label("styled", "Styled").with_style(syntax::visual())),
        (
            "class",
            Node::label("classy", "Classy")
                .with_class(rustnative_core::classes!("p-2 bg-blue-500 hover:bg-blue-600")),
        ),
        (
            "style_declarations",
            Node::label("declared", "Declared")
                .with_declarations(rustnative_core::styles!("padding: 4px; color: #123456")),
        ),
        ("input", Node::column("pad", []).with_input(syntax::interest())),
        ("opacity", Node::label("faint", "Faint").with_opacity(0.5)),
        ("transition", {
            let (property, transition) = syntax::slide();
            Node::column("panel", []).with_transition(property, transition)
        }),
        ("item_index", Node::label("item", "Item").with_item_index(7)),
        ("command", Node::button("save", "Save").with_command(syntax::SAVE)),
        ("cursor", Node::label("link", "Link").with_cursor(rustnative_core::Cursor::Pointer)),
        ("shared_id", Node::label("thumb", "Photo").with_shared_id("photo")),
        ("disabled", Node::button("off", "Off").disabled(true)),
        ("hidden", Node::label("gone", "Gone").hidden(true)),
        (
            "grid",
            Node::grid(
                "form",
                values::grid_tracks(),
                LayoutStyle::new(),
                [
                    Node::label("name-label", "Name"),
                    Node::text_input("name", ""),
                    Node::label_with_layout(
                        "note",
                        "Required",
                        LayoutStyle::new()
                            .grid(rustnative_core::GridPlacement::at(1, 0).span(1, 2)),
                    ),
                ],
            ),
        ),
        ("spread", syntax::emphasized(Node::label("loud", "Loud"))),
        ("control_flow", control_flow(true, 2, &["x", "y"], Some("extra"))),
        ("control_flow_else", control_flow(false, 0, &[], None)),
    ]
}

/// `if`/`else`, `match`, `for`, fragments, and splices, as the builder
/// syntax writes them: ordinary Rust.
#[must_use]
pub fn control_flow(show: bool, count: u8, items: &[&str], extra: Option<&str>) -> Node {
    let mut children = Vec::new();
    if show {
        children.push(Node::label("shown", "Shown"));
    } else {
        children.push(Node::label("hidden-note", "Nothing to show"));
    }
    children.push(match count {
        0 => Node::label("count", "none"),
        1 => Node::label("count", "one"),
        _ => Node::label("count", "many"),
    });
    children.extend(items.iter().map(|item| Node::label(*item, *item)));
    children.push(Node::label("first", "1"));
    children.push(Node::label("second", "2"));
    children.extend(extra.map(|text| Node::label("extra", text)));
    Node::column("flow", children)
}

/// A component element, as the builder syntax composes it.
pub fn components(context: &mut ComponentContext<'_, ()>) -> Node {
    let card = context.child_with_props::<Card, _>(
        "card",
        CardProps { title: "Welcome".into(), highlighted: true },
        Card::new,
    );
    Node::column("cards", [card])
}
