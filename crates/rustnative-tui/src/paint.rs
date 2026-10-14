//! Every node kind as a drawn widget (`PLAN.md` Milestone 38, "drawn, not
//! native": a control is a drawn widget with the portable semantics,
//! identity, and events).
//!
//! The terminal's conventions, not an imitation window's: no title bars,
//! no shadows, no fake bevels. What each widget looks like:
//!
//! | Node | Drawn as |
//! |---|---|
//! | label | its text, wrapped to its width, aligned to the start edge |
//! | button | `[ text ]` — reversed while focused, faint while disabled |
//! | text field | its text, underlined across the field, scrolled to the caret |
//! | check box / radio / switch | `[x] label` / `(*) label` / `[on] label` |
//! | slider | a track `───●───` and the value |
//! | progress | `[█████░░░░░]  60%`, or a moving block while unknown |
//! | select | the choice and `▾`; open, a bordered list below it (above, if no room) |
//! | list box | one item per row, `>` marking the chosen one, the cursor reversed while focused |
//! | date picker | `YYYY-MM-DD`, the focused part reversed |
//! | spinner | `< value >` |
//! | separator | a rule `────` |
//! | link | underlined text |
//! | multi-line text | its text soft-wrapped, scrolled to the caret |
//! | image | half blocks (`▀`, two pixels per cell), or `[image]` with no colour |
//! | tab bar | ` One  Two ` — the chosen tab reversed |
//! | canvas | its draw list rasterized to cells (`rustnative_drawlist::raster`) |
//! | row / column | its background, a box-drawn border when it declares one, and scroll bars |
//!
//! Styles are realized where they differ from the default theme's
//! resolution (the default theme is "the terminal's own look"; see
//! `docs/tui.md`), with the interaction state the node is in.

use std::time::Duration;

use rustnative_core::{
    AnimatedProperty, AnimatedValue, Color, Control, ControlState, LayoutDirection, NodeId,
    NodeKind, Overflow, StyleOverride, Theme,
};
use rustnative_drawlist::{Edges, GridMapping, GridRect, RasterTarget, rasterize};

use crate::color::blend;
use crate::edit::{reveal, visual_lines};
use crate::grid::{Attrs, CellStyle, Grid};
use crate::model::{RealizedNode, TuiModel};
use crate::width::Width;

/// Everything painting needs besides the model.
pub struct PaintContext<'a> {
    /// The application's theme.
    pub theme: &'a Theme,
    /// The theme standing for the terminal's own look.
    pub baseline: &'a Theme,
    /// The cell size.
    pub mapping: GridMapping,
    /// Width rules.
    pub width: Width,
    /// The terminal's default background, when it answered.
    pub background: Option<Color>,
    /// Time, for indeterminate progress.
    pub now: Duration,
    /// Whether motion is reduced (indeterminate progress stands still).
    pub reduced_motion: bool,
    /// Whether colour is available at all (an image needs it).
    pub has_color: bool,
    /// Animated values in force: (node, property) → value.
    pub animated: &'a dyn Fn(NodeId, AnimatedProperty) -> Option<AnimatedValue>,
}

/// Where the terminal's cursor goes after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSpot {
    /// Its cell.
    pub x: i32,
    /// Its row.
    pub y: i32,
    /// Whether it is shown (a caret) or only placed (on a focused control,
    /// for screen readers that follow the cursor).
    pub visible: bool,
}

fn changed(value: Option<Color>, host: Option<Color>) -> Option<Color> {
    if value == host { None } else { value }
}

/// A node's interaction state, as the theme resolves styles for it.
#[must_use]
pub fn state_of(model: &TuiModel, node: &RealizedNode) -> ControlState {
    if node.disabled {
        ControlState::Disabled
    } else if model.pressed() == Some(node.id) {
        ControlState::Pressed
    } else if model.focused() == Some(node.id) {
        ControlState::Focused
    } else if model.hovered() == Some(node.id) {
        ControlState::Hovered
    } else {
        ControlState::Normal
    }
}

/// The resolved style a node is drawn in, and its border colour (if it
/// declares one that differs from the host's).
fn style_of(
    model: &TuiModel,
    node: &RealizedNode,
    context: &PaintContext<'_>,
) -> (CellStyle, Option<Color>) {
    let state = state_of(model, node);
    let resolved = context.theme.resolve(node.kind, state, &node.style_override);
    let host = context.baseline.resolve(node.kind, state, &StyleOverride::default());
    let (style, host) = (resolved.properties(), host.properties());
    let mut fg = changed(style.foreground_override(), host.foreground_override());
    let mut bg = changed(style.background_override(), host.background_override());
    let border = changed(style.border_override(), host.border_override());
    if let Some(AnimatedValue::Color(color)) =
        (context.animated)(node.id, AnimatedProperty::Foreground)
    {
        fg = Some(color);
    }
    if let Some(AnimatedValue::Color(color)) =
        (context.animated)(node.id, AnimatedProperty::Background)
    {
        bg = Some(color);
    }
    let mut attrs = Attrs::NONE;
    let weight = style.typography_override().map(|t| t.weight);
    if weight != host.typography_override().map(|t| t.weight) {
        match weight {
            Some(weight) if weight >= 600 => attrs = attrs.with(Attrs::BOLD),
            Some(weight) if weight <= 300 => attrs = attrs.with(Attrs::DIM),
            _ => {}
        }
    }
    if node.disabled {
        attrs = attrs.with(Attrs::DIM);
    }
    let opacity = match (context.animated)(node.id, AnimatedProperty::Opacity) {
        Some(AnimatedValue::Scalar(value)) => value.get(),
        _ => node.opacity,
    };
    if opacity < 0.5 {
        attrs = attrs.with(Attrs::DIM);
    }
    (CellStyle { fg, bg, attrs }, border)
}

/// The opacity a node is drawn at now (animation included).
fn opacity_of(node: &RealizedNode, context: &PaintContext<'_>) -> f32 {
    match (context.animated)(node.id, AnimatedProperty::Opacity) {
        Some(AnimatedValue::Scalar(value)) => value.get(),
        _ => node.opacity,
    }
}

/// Paints the realized window into `grid` (cleared first) and returns where
/// the cursor goes.
pub fn paint_window(
    model: &mut TuiModel,
    grid: &mut Grid,
    context: &PaintContext<'_>,
) -> Option<CursorSpot> {
    grid.clear(CellStyle::DEFAULT);
    let mut cursor = None;
    let ids: Vec<NodeId> = model.nodes().map(|node| node.id).collect();
    let mut invisible: Vec<NodeId> = Vec::new();
    for id in ids {
        let Some(node) = model.get(id).cloned() else { continue };
        if node.hidden
            || node.visible_cells.is_empty()
            || opacity_of(&node, context) <= 0.0
            || node.parent.is_some_and(|parent| invisible.contains(&parent))
        {
            invisible.push(id);
            continue;
        }
        let spot = paint_node(model, grid, &node, context);
        if model.focused() == Some(id) {
            cursor = spot.or(Some(CursorSpot {
                x: node.visible_cells.x,
                y: node.visible_cells.y,
                visible: false,
            }));
        }
    }
    // An open select's list is drawn over everything else.
    if let Some(focused) = model.focused() {
        if let Some(node) = model.get(focused).cloned() {
            if model.widget(focused).open {
                if let Some(spot) = paint_popup(model, grid, &node, context) {
                    cursor = Some(spot);
                }
            }
        }
    }
    cursor
}

fn print_aligned(
    grid: &mut Grid,
    rect: GridRect,
    row: i32,
    text: &str,
    style: CellStyle,
    context: &PaintContext<'_>,
    rtl: bool,
) {
    let used = i32::try_from(context.width.of(text)).unwrap_or(0);
    let x = if rtl { rect.right() - used } else { rect.x };
    grid.print(x, row, text, style, rect, context.width);
}

fn fill_background(grid: &mut Grid, rect: GridRect, bg: Option<Color>, context: &PaintContext<'_>) {
    let Some(bg) = bg else { return };
    let bounds = rect.intersect(&grid.bounds());
    for y in bounds.y..bounds.bottom() {
        for x in bounds.x..bounds.right() {
            let beneath = grid.get(x, y).and_then(|cell| cell.style.bg).or(context.background);
            let color = match (bg.alpha, beneath) {
                (255, _) => bg,
                (_, Some(beneath)) => blend(bg, beneath),
                (alpha, None) if alpha >= 128 => Color::rgb(bg.red, bg.green, bg.blue),
                _ => continue,
            };
            grid.restyle(GridRect::new(x, y, 1, 1), |style| style.bg(Some(color)));
        }
    }
}

/// The text a node shows over a background, keeping what is beneath.
fn over(grid: &Grid, x: i32, y: i32, style: CellStyle) -> CellStyle {
    if style.bg.is_some() { style } else { style.bg(grid.get(x, y).and_then(|cell| cell.style.bg)) }
}

fn border_glyph(edges: Edges) -> &'static str {
    let (n, e, s, w) = (
        edges.has(Edges::NORTH),
        edges.has(Edges::EAST),
        edges.has(Edges::SOUTH),
        edges.has(Edges::WEST),
    );
    match (n, e, s, w) {
        (false, true, true, false) => "┌",
        (false, false, true, true) => "┐",
        (true, true, false, false) => "└",
        (true, false, false, true) => "┘",
        (true, true, true, true) => "┼",
        (true, true, true, false) => "├",
        (true, false, true, true) => "┤",
        (false, true, true, true) => "┬",
        (true, true, false, true) => "┴",
        (true, false, true, false) | (true, false, false, false) | (false, false, true, false) => {
            "│"
        }
        (false, true, false, true) | (false, true, false, false) | (false, false, false, true) => {
            "─"
        }
        (false, false, false, false) => "·",
    }
}

fn draw_box(
    grid: &mut Grid,
    rect: GridRect,
    style: CellStyle,
    clip: GridRect,
    context: &PaintContext<'_>,
) {
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    let (left, top, right, bottom) = (rect.x, rect.y, rect.right() - 1, rect.bottom() - 1);
    let put = |grid: &mut Grid, x: i32, y: i32, glyph: &str| {
        let style = over(grid, x, y, style);
        grid.print(x, y, glyph, style, clip, context.width);
    };
    for x in left + 1..right {
        put(grid, x, top, "─");
        put(grid, x, bottom, "─");
    }
    for y in top + 1..bottom {
        put(grid, left, y, "│");
        put(grid, right, y, "│");
    }
    put(grid, left, top, "┌");
    put(grid, right, top, "┐");
    put(grid, left, bottom, "└");
    put(grid, right, bottom, "┘");
}

fn draw_scrollbar(
    grid: &mut Grid,
    node: &RealizedNode,
    context: &PaintContext<'_>,
    mapping: GridMapping,
) {
    let rows = node.cells.height;
    let range_rows = mapping.rows(i32::try_from(node.scroll_range.height).unwrap_or(0));
    if rows < 2 || range_rows <= 0 {
        return;
    }
    let column = node.visible_cells.right() - 1;
    let total = rows + range_rows;
    let thumb = (rows * rows / total).max(1);
    let offset_rows = mapping.rows(node.scroll.y).clamp(0, range_rows);
    let position = if range_rows == 0 { 0 } else { offset_rows * (rows - thumb) / range_rows };
    for row in 0..rows {
        let y = node.cells.y + row;
        let glyph = if row >= position && row < position + thumb { "█" } else { "│" };
        let style = over(
            grid,
            column,
            y,
            CellStyle::DEFAULT.attrs(if glyph == "│" { Attrs::DIM } else { Attrs::NONE }),
        );
        grid.print(column, y, glyph, style, node.visible_cells, context.width);
    }
}

/// The columns each tab takes, relative to the bar: `(start, end)`.
#[must_use]
pub fn tab_spans(labels: &[String], width: Width) -> Vec<(i32, i32)> {
    let mut spans = Vec::new();
    let mut x = 0;
    for label in labels {
        let w = i32::try_from(width.of(label)).unwrap_or(0) + 2;
        spans.push((x, x + w));
        x += w + 1;
    }
    spans
}

/// The cells a slider's track takes, relative to its left edge: `(start,
/// length)`.
#[must_use]
pub fn slider_track(node: &RealizedNode, min: i64, max: i64) -> (i32, i32) {
    let digits = i32::try_from(min.to_string().len().max(max.to_string().len())).unwrap_or(1);
    (0, (node.cells.width - digits - 1).max(1))
}

/// A slider's value at column `column` of its track.
#[must_use]
pub fn slider_value_at(node: &RealizedNode, min: i64, max: i64, column: i32) -> i64 {
    let (_, length) = slider_track(node, min, max);
    if length <= 1 || max <= min {
        return min;
    }
    let column = i64::from(column.clamp(0, length - 1));
    min + ((max - min) * column + i64::from(length - 1) / 2) / i64::from(length - 1)
}

/// An open select's list: its frame (with a border) and how many items it
/// shows, below the select or above it when below has no room.
#[must_use]
pub fn popup_frame(
    node: &RealizedNode,
    items: &[String],
    window: GridRect,
    width: Width,
) -> GridRect {
    let shown = i32::try_from(items.len().clamp(1, 8)).unwrap_or(8);
    let widest =
        items.iter().map(|item| i32::try_from(width.of(item)).unwrap_or(0)).max().unwrap_or(0);
    let frame_width = (widest + 4).max(node.cells.width).min(window.width);
    let height = shown + 2;
    let below = node.cells.bottom();
    let y = if below + height <= window.bottom() {
        below
    } else {
        (node.cells.y - height).max(window.y)
    };
    let x = node.cells.x.min(window.right() - frame_width).max(window.x);
    GridRect::new(x, y, frame_width, height.min(window.height))
}

/// The first item an open select's list shows, keeping `cursor` in view.
#[must_use]
pub fn popup_first(count: usize, cursor: usize, rows: usize) -> usize {
    if rows == 0 || count <= rows { 0 } else { cursor.saturating_sub(rows - 1).min(count - rows) }
}

/// The first item a list box shows, keeping `cursor` in view from
/// `first`.
#[must_use]
pub fn list_first(first: usize, cursor: Option<usize>, rows: usize, count: usize) -> usize {
    let rows = rows.max(1);
    let mut first = first.min(count.saturating_sub(rows));
    if let Some(cursor) = cursor {
        if cursor < first {
            first = cursor;
        } else if cursor >= first + rows {
            first = cursor + 1 - rows;
        }
    }
    first
}

fn paint_popup(
    model: &TuiModel,
    grid: &mut Grid,
    node: &RealizedNode,
    context: &PaintContext<'_>,
) -> Option<CursorSpot> {
    let Some(Control::Select { options, selected }) = &node.control else { return None };
    let frame = popup_frame(node, options, grid.bounds(), context.width);
    let all = grid.bounds();
    grid.fill(frame, CellStyle::DEFAULT.bg(context.background));
    draw_box(grid, frame, CellStyle::DEFAULT, all, context);
    let rows = usize::try_from(frame.height - 2).unwrap_or(0);
    let cursor = model.widget(node.id).cursor.or(*selected).unwrap_or(0);
    let first = popup_first(options.len(), cursor, rows);
    let inner = GridRect::new(frame.x + 1, frame.y + 1, frame.width - 2, frame.height - 2);
    let mut spot = None;
    for (row, (index, option)) in options.iter().enumerate().skip(first).take(rows).enumerate() {
        let y = inner.y + i32::try_from(row).unwrap_or(0);
        let mut style = CellStyle::DEFAULT;
        if Some(index) == *selected {
            style = style.attrs(Attrs::BOLD);
        }
        if index == cursor {
            style = style.attrs(Attrs::REVERSE);
            grid.fill(GridRect::new(inner.x, y, inner.width, 1), style);
            spot = Some(CursorSpot { x: inner.x, y, visible: false });
        }
        grid.print(inner.x + 1, y, option, style, inner, context.width);
    }
    spot
}

struct CellTarget<'a, 'b> {
    grid: &'a mut Grid,
    clip: GridRect,
    context: &'a PaintContext<'b>,
}

impl RasterTarget for CellTarget<'_, '_> {
    fn bounds(&self) -> GridRect {
        self.clip
    }

    fn fill(&mut self, x: i32, y: i32, color: Color) {
        let beneath =
            self.grid.get(x, y).and_then(|cell| cell.style.bg).or(self.context.background);
        let color = match beneath {
            Some(beneath) if color.alpha < 255 => blend(color, beneath),
            _ => Color::rgb(color.red, color.green, color.blue),
        };
        self.grid.fill(GridRect::new(x, y, 1, 1), CellStyle::DEFAULT.bg(Some(color)));
    }

    fn stroke(&mut self, x: i32, y: i32, edges: Edges, color: Color) {
        let style = over(
            self.grid,
            x,
            y,
            CellStyle::DEFAULT.fg(Some(Color::rgb(color.red, color.green, color.blue))),
        );
        self.grid.print(x, y, border_glyph(edges), style, self.clip, self.context.width);
    }

    fn text(&mut self, x: i32, y: i32, text: &str, _rows: f32, color: Color, clip: GridRect) {
        let mut column = x;
        for grapheme in crate::width::graphemes(text) {
            let style = over(
                self.grid,
                column,
                y,
                CellStyle::DEFAULT.fg(Some(Color::rgb(color.red, color.green, color.blue))),
            );
            column += self.grid.print(column, y, grapheme, style, clip, self.context.width);
        }
    }

    fn image(&mut self, x: i32, y: i32, upper: Color, lower: Color) {
        let beneath = self
            .grid
            .get(x, y)
            .and_then(|cell| cell.style.bg)
            .or(self.context.background)
            .unwrap_or(Color::rgb(0, 0, 0));
        let (upper, lower) = (blend(upper, beneath), blend(lower, beneath));
        let style = CellStyle::DEFAULT.fg(Some(upper)).bg(Some(lower));
        self.grid.print(x, y, "▀", style, self.clip, self.context.width);
    }
}

#[allow(clippy::too_many_lines, reason = "one arm per node kind")]
fn paint_node(
    model: &mut TuiModel,
    grid: &mut Grid,
    node: &RealizedNode,
    context: &PaintContext<'_>,
) -> Option<CursorSpot> {
    let (style, border) = style_of(model, node, context);
    let clip = node.visible_cells;
    let rect = node.cells;
    let rtl = model.direction() == LayoutDirection::Rtl;
    let focused = model.focused() == Some(node.id);
    fill_background(grid, rect.intersect(&clip), style.bg, context);
    let text_style =
        |grid: &Grid, x: i32, y: i32| over(grid, x, y, CellStyle { bg: style.bg, ..style });
    match node.kind {
        NodeKind::Column | NodeKind::Row | NodeKind::Canvas | NodeKind::Surface => {
            if let Some(border) = border {
                draw_box(grid, rect, CellStyle::DEFAULT.fg(Some(border)), clip, context);
            }
            if node.kind == NodeKind::Canvas {
                if let Some(list) = &node.draw_list {
                    let mut target = CellTarget { grid, clip, context };
                    let _ = rasterize(list, rect, context.mapping, &mut target);
                }
            }
            if node.overflow == Some(Overflow::Scroll) && node.scroll_range.height > 0 {
                draw_scrollbar(grid, node, context, context.mapping);
            }
            None
        }
        NodeKind::Label => {
            let text = node.text.clone().unwrap_or_default();
            let columns = usize::try_from(rect.width.max(1)).unwrap_or(1);
            for (row, line) in context.width.wrap(&text, columns).iter().enumerate() {
                let y = rect.y + i32::try_from(row).unwrap_or(0);
                let style = text_style(grid, rect.x, y);
                print_aligned(grid, rect, y, line, style, context, rtl);
            }
            None
        }
        NodeKind::Button => {
            let text = node.text.clone().unwrap_or_default();
            let y = rect.y + (rect.height - 1) / 2;
            let inner = usize::try_from((rect.width - 4).max(0)).unwrap_or(0);
            let (label, used) = context.width.truncate(&text, inner);
            let mut look = text_style(grid, rect.x, y);
            if focused {
                look = look.attrs(Attrs::REVERSE);
            }
            if model.pressed() == Some(node.id) {
                look = look.attrs(Attrs::BOLD);
            }
            let padding = (inner - used.min(inner)) / 2;
            let content = format!(
                "[ {}{}{} ]",
                " ".repeat(padding),
                label,
                " ".repeat(inner - used.min(inner) - padding)
            );
            grid.print(rect.x, y, &content, look, clip, context.width);
            Some(CursorSpot { x: rect.x + 2, y, visible: false })
        }
        NodeKind::TextInput => {
            let value = model.value(node.id);
            let columns = usize::try_from(rect.width.max(1)).unwrap_or(1);
            let y = rect.y + (rect.height - 1) / 2;
            let mut widget = model.widget(node.id);
            let (caret_x, _) = reveal(
                &value,
                &mut widget.edit,
                columns.saturating_sub(1).max(1),
                1,
                false,
                context.width,
            );
            *model.widget_mut(node.id) = widget;
            let look = text_style(grid, rect.x, y).attrs(Attrs::UNDERLINE);
            grid.fill(GridRect::new(rect.x, y, rect.width, 1), look);
            draw_field_line(
                grid,
                rect.x,
                y,
                &value,
                0,
                value.len(),
                widget.edit.scroll,
                columns,
                &widget,
                look,
                clip,
                context,
                focused,
            );
            focused.then(|| CursorSpot {
                x: rect.x + i32::try_from(caret_x).unwrap_or(0),
                y,
                visible: true,
            })
        }
        NodeKind::TabBar => {
            let tabs = node.tabs.clone()?;
            let y = rect.y + (rect.height - 1) / 2;
            let spans = tab_spans(tabs.labels(), context.width);
            let mut spot = None;
            for (index, (label, (start, _))) in tabs.labels().iter().zip(&spans).enumerate() {
                let mut look = text_style(grid, rect.x + start, y);
                if index == tabs.selected() {
                    look = look.attrs(Attrs::REVERSE);
                    if focused {
                        look = look.attrs(Attrs::BOLD).attrs(Attrs::UNDERLINE);
                        spot = Some(CursorSpot { x: rect.x + start, y, visible: false });
                    }
                }
                grid.print(rect.x + start, y, &format!(" {label} "), look, clip, context.width);
            }
            spot
        }
        NodeKind::Control => {
            let control = node.control.clone()?;
            paint_control(model, grid, node, &control, style, focused, context)
        }
    }
}

#[allow(clippy::too_many_arguments, reason = "a line of a field and everything it is drawn with")]
fn draw_field_line(
    grid: &mut Grid,
    x: i32,
    y: i32,
    value: &str,
    start: usize,
    end: usize,
    scroll_columns: usize,
    columns: usize,
    widget: &crate::model::WidgetState,
    style: CellStyle,
    clip: GridRect,
    context: &PaintContext<'_>,
    focused: bool,
) {
    let selection = widget.edit.selection();
    let mut column = 0_usize;
    let mut offset = start;
    for grapheme in crate::width::graphemes(&value[start..end]) {
        let cells = context.width.of_grapheme(grapheme);
        if column >= scroll_columns && column + cells <= scroll_columns + columns {
            let mut look = style;
            if focused && selection.contains(&offset) {
                look = look.attrs(Attrs::REVERSE);
            }
            let cell_x = x + i32::try_from(column - scroll_columns).unwrap_or(0);
            grid.print(cell_x, y, grapheme, look, clip, context.width);
        }
        column += cells;
        offset += grapheme.len();
    }
}

#[allow(clippy::too_many_lines, reason = "one arm per control")]
fn paint_control(
    model: &mut TuiModel,
    grid: &mut Grid,
    node: &RealizedNode,
    control: &Control,
    style: CellStyle,
    focused: bool,
    context: &PaintContext<'_>,
) -> Option<CursorSpot> {
    let rect = node.cells;
    let clip = node.visible_cells;
    let y = rect.y + (rect.height - 1) / 2;
    let base = over(grid, rect.x, y, style);
    let focus = |look: CellStyle| if focused { look.attrs(Attrs::REVERSE) } else { look };
    let spot = |x: i32| Some(CursorSpot { x, y, visible: false });
    match control {
        Control::Checkbox { label, checked } => {
            let mark = if *checked { "[x]" } else { "[ ]" };
            grid.print(rect.x, y, mark, focus(base), clip, context.width);
            grid.print(rect.x + 4, y, label, base, clip, context.width);
            spot(rect.x + 1)
        }
        Control::Radio { label, selected } => {
            let mark = if *selected { "(*)" } else { "( )" };
            grid.print(rect.x, y, mark, focus(base), clip, context.width);
            grid.print(rect.x + 4, y, label, base, clip, context.width);
            spot(rect.x + 1)
        }
        Control::Toggle { label, on } => {
            let mark = if *on { "[on] " } else { "[off]" };
            grid.print(rect.x, y, mark, focus(base), clip, context.width);
            grid.print(rect.x + 6, y, label, base, clip, context.width);
            spot(rect.x + 1)
        }
        Control::Slider { value, min, max } => {
            let (_, length) = slider_track(node, *min, *max);
            let span = (max - min).max(1);
            let knob = i32::try_from((value.clamp(min, max) - min) * i64::from(length - 1) / span)
                .unwrap_or(0);
            for column in 0..length {
                let glyph = if column == knob {
                    "●"
                } else if column < knob {
                    "━"
                } else {
                    "─"
                };
                let look = if column == knob { focus(base) } else { base };
                grid.print(rect.x + column, y, glyph, look, clip, context.width);
            }
            grid.print(rect.x + length + 1, y, &value.to_string(), base, clip, context.width);
            spot(rect.x + knob)
        }
        Control::Progress { percent } => {
            let inner = (rect.width - 7).max(1);
            grid.print(rect.x, y, "[", base, clip, context.width);
            grid.print(rect.x + inner + 1, y, "]", base, clip, context.width);
            match percent {
                Some(percent) => {
                    let filled = i32::from(*percent).min(100) * inner / 100;
                    for column in 0..inner {
                        grid.print(
                            rect.x + 1 + column,
                            y,
                            if column < filled { "█" } else { "░" },
                            base,
                            clip,
                            context.width,
                        );
                    }
                    grid.print(
                        rect.x + inner + 2,
                        y,
                        &format!("{percent:>3}%"),
                        base,
                        clip,
                        context.width,
                    );
                }
                None => {
                    let block = (inner / 4).max(1);
                    let position = if context.reduced_motion {
                        0
                    } else {
                        let step = i32::try_from(
                            context.now.as_millis() / 100
                                % u128::try_from((inner + block).max(1)).unwrap_or(1),
                        )
                        .unwrap_or(0);
                        step - block
                    };
                    for column in 0..inner {
                        let lit = column >= position && column < position + block;
                        grid.print(
                            rect.x + 1 + column,
                            y,
                            if lit { "█" } else { " " },
                            base,
                            clip,
                            context.width,
                        );
                    }
                    grid.print(rect.x + inner + 2, y, "  …", base, clip, context.width);
                }
            }
            None
        }
        Control::Select { options, selected } => {
            let text = selected.and_then(|index| options.get(index)).cloned().unwrap_or_default();
            let look = focus(base.attrs(Attrs::UNDERLINE));
            grid.fill(GridRect::new(rect.x, y, rect.width, 1), look);
            let (shown, _) = context
                .width
                .truncate(&text, usize::try_from((rect.width - 2).max(0)).unwrap_or(0));
            grid.print(rect.x, y, &shown, look, clip, context.width);
            grid.print(rect.right() - 1, y, "▾", look, clip, context.width);
            spot(rect.x)
        }
        Control::ListBox { items, selected } => {
            let rows = usize::try_from(rect.height.max(1)).unwrap_or(1);
            let widget = model.widget(node.id);
            let first = list_first(widget.first, widget.cursor, rows, items.len());
            model.widget_mut(node.id).first = first;
            let scrollbar = items.len() > rows;
            let text_width = rect.width - 1 - i32::from(scrollbar);
            let mut cursor_spot = spot(rect.x);
            for (row, index) in (first..items.len()).take(rows).enumerate() {
                let y = rect.y + i32::try_from(row).unwrap_or(0);
                let mut look = over(grid, rect.x, y, style);
                if Some(index) == *selected {
                    look = look.attrs(Attrs::BOLD);
                }
                if focused && widget.cursor == Some(index) {
                    look = look.attrs(Attrs::REVERSE);
                    grid.fill(GridRect::new(rect.x, y, text_width + 1, 1).intersect(&clip), look);
                    cursor_spot = Some(CursorSpot { x: rect.x, y, visible: false });
                }
                let marker = if Some(index) == *selected { ">" } else { " " };
                grid.print(rect.x, y, marker, look, clip, context.width);
                let (item, _) = context
                    .width
                    .truncate(&items[index], usize::try_from(text_width.max(0)).unwrap_or(0));
                grid.print(
                    rect.x + 1,
                    y,
                    &item,
                    look,
                    GridRect::new(rect.x, y, text_width + 1, 1).intersect(&clip),
                    context.width,
                );
            }
            if scrollbar {
                let total = i32::try_from(items.len()).unwrap_or(1);
                let rows_i = i32::try_from(rows).unwrap_or(1);
                let thumb = (rows_i * rows_i / total).max(1);
                let range = (total - rows_i).max(1);
                let position = i32::try_from(first).unwrap_or(0) * (rows_i - thumb) / range;
                for row in 0..rows_i {
                    let glyph =
                        if row >= position && row < position + thumb { "█" } else { "│" };
                    grid.print(
                        rect.right() - 1,
                        rect.y + row,
                        glyph,
                        over(grid, rect.right() - 1, rect.y + row, CellStyle::DEFAULT),
                        clip,
                        context.width,
                    );
                }
            }
            cursor_spot
        }
        Control::DatePicker { date } => {
            let parts = [
                format!("{:04}", date.year),
                format!("{:02}", date.month),
                format!("{:02}", date.day),
            ];
            let segment = usize::from(model.widget(node.id).segment.min(2));
            let mut x = rect.x;
            let mut cursor = rect.x;
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    grid.print(x, y, "-", base, clip, context.width);
                    x += 1;
                }
                let look = if focused && index == segment {
                    base.attrs(Attrs::REVERSE)
                } else {
                    base.attrs(Attrs::UNDERLINE)
                };
                if index == segment {
                    cursor = x;
                }
                grid.print(x, y, part, look, clip, context.width);
                x += i32::try_from(part.len()).unwrap_or(0);
            }
            spot(cursor)
        }
        Control::Spinner { value, .. } => {
            let inner = usize::try_from((rect.width - 4).max(1)).unwrap_or(1);
            let text = format!("< {:>inner$} >", value);
            grid.print(rect.x, y, &text, focus(base), clip, context.width);
            spot(rect.x + 2)
        }
        Control::Separator => {
            for column in 0..rect.width {
                grid.print(rect.x + column, y, "─", base.attrs(Attrs::DIM), clip, context.width);
            }
            None
        }
        Control::Link { text } => {
            let look = focus(base.attrs(Attrs::UNDERLINE));
            grid.print(rect.x, y, text, look, clip, context.width);
            spot(rect.x)
        }
        Control::MultilineText { .. } => {
            let value = model.value(node.id);
            let columns = usize::try_from(rect.width.max(1)).unwrap_or(1);
            let rows = usize::try_from(rect.height.max(1)).unwrap_or(1);
            let mut widget = model.widget(node.id);
            let (caret_x, caret_y) =
                reveal(&value, &mut widget.edit, columns, rows, true, context.width);
            *model.widget_mut(node.id) = widget;
            let look = over(grid, rect.x, rect.y, style);
            let lines = visual_lines(&value, columns, context.width, true);
            for (row, line) in lines.iter().skip(widget.edit.scroll).take(rows).enumerate() {
                let y = rect.y + i32::try_from(row).unwrap_or(0);
                draw_field_line(
                    grid, rect.x, y, &value, line.start, line.end, 0, columns, &widget, look, clip,
                    context, focused,
                );
            }
            focused.then(|| CursorSpot {
                x: rect.x + i32::try_from(caret_x).unwrap_or(0),
                y: rect.y + i32::try_from(caret_y).unwrap_or(0),
                visible: true,
            })
        }
        Control::Image { image } => {
            if context.has_color {
                let list = rustnative_core::DrawList::new().image(
                    image.clone(),
                    rustnative_core::RectF::new(
                        0.0,
                        0.0,
                        f32::from(u16::try_from(node.rect.width.max(0)).unwrap_or(u16::MAX)),
                        f32::from(u16::try_from(node.rect.height.max(0)).unwrap_or(u16::MAX)),
                    ),
                );
                let mut target = CellTarget { grid, clip, context };
                let _ = rasterize(&list, rect, context.mapping, &mut target);
            } else {
                let name = node.name.clone().unwrap_or_else(|| "image".to_owned());
                grid.print(rect.x, y, &format!("[{name}]"), base, clip, context.width);
            }
            None
        }
        _ => None,
    }
}
