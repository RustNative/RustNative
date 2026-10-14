//! `rustnative_style::TUI` is what the grid shows (`docs/superpowers/plans/
//! 2026-10-13-terminal-milestone-38.md`, Phase 2): every property the table
//! answers as realized or approximated changes the screen when it is
//! declared, and every property it answers as unavailable changes nothing —
//! so the table can be neither optimistic nor stale.

mod common;

use rustnative_core::{
    Alignment, Color, ColumnStyle, EdgeInsets, LayoutStyle, Node, Overflow, ShadowLayer, SizeMode,
    StyleProperty, StyleSupport, Typography, VisualStyle,
};
use rustnative_tui::TuiHarness;
use rustnative_tui::grid::Grid;

const RED: Color = Color::rgb(200, 30, 30);

fn boxed(key: &str, color: Color, layout: LayoutStyle) -> Node {
    Node::column_with_layout(key, [], layout, ColumnStyle::new().padding(EdgeInsets::all(0)).gap(0))
        .with_style(VisualStyle::new().background(color))
}

/// The screen with `property` declared somewhere it can show (or with
/// nothing declared).
#[allow(clippy::too_many_lines, reason = "one arm per property")]
fn view(property: Option<StyleProperty>) -> Node {
    use StyleProperty as P;
    let mut label = Node::label("label", "Styled text");
    let mut label_style = VisualStyle::new();
    let box_layout = LayoutStyle::new().width(SizeMode::Fixed(80)).height(SizeMode::Fixed(32));
    let mut a_layout = box_layout;
    let mut parent_layout = LayoutStyle::new().height(SizeMode::Fixed(96));
    let mut parent_style =
        ColumnStyle::new().padding(EdgeInsets::all(8)).gap(0).align_items(Alignment::Start);
    let mut parent_visual = VisualStyle::new();
    let mut hidden = false;
    let mut opacity = 1.0;
    let typography =
        |family: &str, size: u16, weight: u16| Typography { family: family.into(), size, weight };
    match property {
        None => {}
        Some(P::Foreground) => label_style = label_style.foreground(RED),
        Some(P::Background) => label_style = label_style.background(RED),
        Some(P::BorderColor) => parent_visual = parent_visual.border(RED),
        Some(P::BorderRadius) => parent_visual = parent_visual.border_radius(6),
        Some(P::FontSize) => label_style = label_style.typography(typography("system-ui", 30, 400)),
        Some(P::FontWeight) => {
            label_style = label_style.typography(typography("system-ui", 14, 700))
        }
        Some(P::FontFamily) => label_style = label_style.typography(typography("serif", 14, 400)),
        Some(P::Shadow) => {
            parent_visual = parent_visual.shadow(vec![ShadowLayer {
                x: rustnative_style::Fixed::from_int(2),
                y: rustnative_style::Fixed::from_int(2),
                blur: rustnative_style::Fixed::from_int(4),
                spread: rustnative_style::Fixed::ZERO,
                color: RED,
                inset: false,
            }]);
        }
        Some(P::PaddingTop) => parent_style.padding.top = 40,
        Some(P::PaddingBottom) => {
            parent_style.padding.bottom = 60;
            parent_layout = LayoutStyle::new();
        }
        Some(P::PaddingStart) => parent_style.padding.start = 40,
        Some(P::PaddingEnd) => {
            parent_style = parent_style.align_items(Alignment::End);
            parent_style.padding.end = 40;
        }
        Some(P::MarginTop) => {
            a_layout = a_layout.margin(EdgeInsets { top: 32, ..EdgeInsets::all(0) })
        }
        Some(P::MarginBottom) => {
            a_layout = a_layout.margin(EdgeInsets { bottom: 32, ..EdgeInsets::all(0) })
        }
        Some(P::MarginStart) => {
            a_layout = a_layout.margin(EdgeInsets { start: 32, ..EdgeInsets::all(0) })
        }
        Some(P::MarginEnd) => {
            parent_style = parent_style.align_items(Alignment::End);
            a_layout = a_layout.margin(EdgeInsets { end: 32, ..EdgeInsets::all(0) });
        }
        Some(P::Width) => a_layout = a_layout.width(SizeMode::Fixed(160)),
        Some(P::Height) => a_layout = a_layout.height(SizeMode::Fixed(48)),
        Some(P::MinWidth) => {
            a_layout = a_layout.constraints(rustnative_core::Constraints::new().with_min_width(200))
        }
        Some(P::MinHeight) => {
            a_layout = a_layout.constraints(rustnative_core::Constraints::new().with_min_height(64))
        }
        Some(P::MaxWidth) => {
            a_layout = a_layout.constraints(rustnative_core::Constraints::new().with_max_width(40))
        }
        Some(P::MaxHeight) => {
            a_layout = a_layout.constraints(rustnative_core::Constraints::new().with_max_height(16))
        }
        Some(P::Gap) => parent_style = parent_style.gap(16),
        Some(P::AlignItems) => parent_style = parent_style.align_items(Alignment::Center),
        Some(P::AlignSelf) => a_layout = a_layout.align_self(Alignment::End),
        Some(P::Overflow) => parent_style = parent_style.overflow(Overflow::Visible),
        Some(P::Opacity) => opacity = 0.3,
        Some(P::Display) => hidden = true,
    }
    label = label.with_style(label_style).with_opacity(opacity).hidden(hidden);
    let parent = Node::column_with_layout(
        "parent",
        [
            boxed("a", Color::rgb(30, 30, 200), a_layout),
            boxed("b", Color::rgb(30, 160, 30), box_layout),
            boxed("c", Color::rgb(160, 160, 30), box_layout),
        ],
        parent_layout,
        parent_style,
    )
    .with_style(parent_visual);
    common::dense("root", [label, parent])
}

fn screen(property: Option<StyleProperty>) -> Grid {
    struct Screen(Option<StyleProperty>);
    impl rustnative_core::Component for Screen {
        type Props = Option<StyleProperty>;
        type Message = ();
        fn new(property: Option<StyleProperty>) -> Self {
            Self(property)
        }
        fn props(&self) -> &Option<StyleProperty> {
            &self.0
        }
        fn set_props(&mut self, property: Option<StyleProperty>) {
            self.0 = property;
        }
        fn view(&self) -> Node {
            view(self.0)
        }
        fn update(&mut self, _event: rustnative_core::Event) {}
    }
    TuiHarness::launch(common::window("style"), 40, 16, move || Screen(property)).screen()
}

#[test]
fn the_tui_capability_table_is_what_the_grid_shows() {
    let plain = screen(None);
    let mut wrong = Vec::new();
    for (property, support) in rustnative_style::TUI.rows() {
        let changed = screen(Some(property)) != plain;
        let claimed = !matches!(support, StyleSupport::Unavailable(_));
        if changed != claimed {
            wrong.push(format!(
                "`{property}` is answered {support:?} but declaring it {} the screen",
                if changed { "changes" } else { "does not change" }
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_unit_mapping_is_the_conversion_the_backend_applies() {
    // 8 × 16 logical pixels a cell, half away from zero, a nonzero length
    // at least one cell: a 12-pixel gap is one row, a 24-pixel padding two
    // rows and three columns.
    let mapping = rustnative_drawlist::GridMapping::new(8, 16);
    assert_eq!(mapping.rows(12), 1);
    assert_eq!(mapping.rows(24), 2);
    assert_eq!(mapping.columns(24), 3);
    assert_eq!(mapping.columns(1), 1);
    let units = rustnative_style::TUI_UNITS;
    assert_eq!(units.host_unit, "character cells");
    assert!(units.pixel.contains("8 logical pixels wide and 16 high"));
    assert!(units.rounding.contains("half away from zero"));
    assert!(units.rounding.contains("at least one cell"));
}
