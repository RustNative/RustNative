//! Charts on the draw-list path (`PLAN.md` Milestone 48): line, area, bar,
//! scatter, and pie — each with an accessible alternative rather than an
//! image with a label.
//!
//! A chart is a canvas, and its drawing is only one of two encodings of its
//! data. The other is in the accessibility tree: the canvas is a table
//! whose cells are the data points, each with a name a screen reader
//! speaks ("Revenue, March: 42"), and its description is a summary of the
//! whole chart. The chart takes keyboard focus; the arrow keys move from
//! point to point (Left/Right along a series, Up/Down between series), and
//! the focused point is shown beside the chart and announced.
//!
//! Axis ticks are "nice numbers": 1, 2, or 5 times a power of ten.

use rustnative_core::{
    AccessibilityInfo, AccessibilityRole, Color, Component, DrawList, Event, KeyCode, LayoutStyle,
    LiveRegion, Node, Paint, Path, Rect, RectF, SizeMode, Vec2, VirtualElement,
};

/// What kind of chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartKind {
    /// Points joined by lines.
    #[default]
    Line,
    /// A line chart filled to the axis.
    Area,
    /// Bars side by side per category.
    Bar,
    /// Points alone.
    Scatter,
    /// The first series as wedges of a circle.
    Pie,
}

/// One series of values, one per category.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Series {
    /// Its name, in the legend and in each point's announcement.
    pub name: String,
    /// Its values, one per category.
    pub values: Vec<f64>,
}

impl Series {
    /// A series named `name`.
    pub fn new(name: impl Into<String>, values: impl IntoIterator<Item = f64>) -> Self {
        Self { name: name.into(), values: values.into_iter().collect() }
    }
}

/// What a [`Chart`] shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChartProps {
    /// The kind of chart.
    pub kind: ChartKind,
    /// Its title, which is also its accessible name.
    pub title: String,
    /// The category of each value.
    pub categories: Vec<String>,
    /// The series.
    pub series: Vec<Series>,
    /// Its width in pixels (default 360).
    pub width: u32,
    /// Its height in pixels (default 220).
    pub height: u32,
}

/// The palette series take their colors from, in order: distinct in hue
/// and lightness, so series stay apart for people who do not see color.
pub const PALETTE: [Color; 6] = [
    Color::rgb(0x1f, 0x6f, 0xd1),
    Color::rgb(0xe0, 0x7a, 0x10),
    Color::rgb(0x2e, 0x9e, 0x5b),
    Color::rgb(0xc2, 0x3b, 0x5a),
    Color::rgb(0x6d, 0x4c, 0xc2),
    Color::rgb(0x55, 0x55, 0x55),
];

/// Tick values covering `low..=high` in about `count` steps of 1, 2, or 5
/// times a power of ten.
///
/// ```
/// use rustnative_components::chart::nice_ticks;
///
/// assert_eq!(nice_ticks(0.0, 97.0, 5), vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
/// assert_eq!(nice_ticks(0.0, 1.3, 4), vec![0.0, 0.5, 1.0, 1.5]);
/// ```
#[must_use]
pub fn nice_ticks(low: f64, high: f64, count: usize) -> Vec<f64> {
    let (low, high) = if high > low { (low, high) } else { (low - 1.0, low + 1.0) };
    #[allow(clippy::cast_precision_loss, reason = "a tick count is tiny")]
    let rough = (high - low) / count.max(1) as f64;
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * magnitude)
        .find(|step| *step >= rough)
        .unwrap_or(magnitude * 10.0);
    let first = (low / step).floor() * step;
    let mut ticks = Vec::new();
    let mut tick = first;
    while tick <= high + step * 0.5 && ticks.len() < 64 {
        // Rounded so 0.1 + 0.2 reads as 0.3.
        ticks.push((tick / step).round() * step);
        if tick >= high {
            break;
        }
        tick += step;
    }
    ticks
}

fn number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 { format!("{value:.0}") } else { format!("{value:.2}") }
}

/// A rectangle in plain `f32`s, for the chart's own geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Area {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Area {
    const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    fn rect(self) -> RectF {
        RectF::new(self.x, self.y, self.width, self.height)
    }
}

/// A chart; see the [module documentation](crate::chart).
#[derive(Debug)]
pub struct Chart {
    props: ChartProps,
    focused: Option<(usize, usize)>,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixel geometry of non-negative, bounded values"
)]
impl Chart {
    fn size(&self) -> (f32, f32) {
        let width = if self.props.width == 0 { 360 } else { self.props.width };
        let height = if self.props.height == 0 { 220 } else { self.props.height };
        (width as f32, height as f32)
    }

    fn range(&self) -> (f64, f64) {
        let values = self.props.series.iter().flat_map(|series| series.values.iter().copied());
        let (low, high) = values
            .fold((0.0_f64, f64::MIN), |(low, high), value| (low.min(value), high.max(value)));
        let ticks = nice_ticks(low, high.max(low + 1.0), 5);
        (*ticks.first().unwrap_or(&low), *ticks.last().unwrap_or(&high))
    }

    /// The plot area: inside the axis labels.
    fn plot(&self) -> Area {
        let (width, height) = self.size();
        Area::new(40.0, 10.0, width - 50.0, height - 40.0)
    }

    /// Where value `index` of series `series` is drawn.
    fn point(&self, series: usize, index: usize) -> Area {
        let plot = self.plot();
        let count = self.props.categories.len().max(1);
        let (low, high) = self.range();
        let value = self.props.series[series].values.get(index).copied().unwrap_or(0.0);
        let y = plot.y + plot.height - ((value - low) / (high - low)) as f32 * plot.height;
        let slot = plot.width / count as f32;
        if self.props.kind == ChartKind::Bar {
            let bars = self.props.series.len().max(1) as f32;
            let width = slot * 0.8 / bars;
            let x = plot.x + slot * index as f32 + slot * 0.1 + width * series as f32;
            let zero = plot.y + plot.height - ((0.0 - low) / (high - low)) as f32 * plot.height;
            Area::new(x, y.min(zero), width, (zero - y).abs())
        } else {
            let x = plot.x + slot * (index as f32 + 0.5);
            Area::new(x - 4.0, y - 4.0, 8.0, 8.0)
        }
    }

    fn draw(&self) -> DrawList {
        let (width, height) = self.size();
        let plot = self.plot();
        let axis = Paint::color(Color::rgb(0x88, 0x88, 0x88)).stroke_width(1.0);
        let text = Color::rgb(0x44, 0x44, 0x44);
        let mut list = DrawList::new().fill_rect(
            RectF::new(0.0, 0.0, width, height),
            Paint::color(Color::rgb(255, 255, 255)),
        );
        if self.props.kind == ChartKind::Pie {
            return self.draw_pie(list);
        }
        let (low, high) = self.range();
        for tick in nice_ticks(low, high, 5) {
            let y = plot.y + plot.height - ((tick - low) / (high - low)) as f32 * plot.height;
            list = list
                .stroke_line(
                    Vec2::new(plot.x, y),
                    Vec2::new(plot.x + plot.width, y),
                    Paint::color(Color::rgb(0xe4, 0xe4, 0xe4)).stroke_width(1.0),
                )
                .text(Vec2::new(4.0, y - 6.0), number(tick), 10.0, text);
        }
        list = list
            .stroke_line(Vec2::new(plot.x, plot.y), Vec2::new(plot.x, plot.y + plot.height), axis)
            .stroke_line(
                Vec2::new(plot.x, plot.y + plot.height),
                Vec2::new(plot.x + plot.width, plot.y + plot.height),
                axis,
            );
        let slot = plot.width / self.props.categories.len().max(1) as f32;
        for (index, category) in self.props.categories.iter().enumerate() {
            list = list.text(
                Vec2::new(plot.x + slot * index as f32 + 4.0, plot.y + plot.height + 6.0),
                category.clone(),
                10.0,
                text,
            );
        }
        for (series, data) in self.props.series.iter().enumerate() {
            let color = PALETTE[series % PALETTE.len()];
            let points =
                (0..data.values.len()).map(|index| self.point(series, index)).collect::<Vec<_>>();
            match self.props.kind {
                ChartKind::Bar => {
                    for rect in points {
                        list = list.fill_rect(rect.rect(), Paint::color(color));
                    }
                }
                ChartKind::Line | ChartKind::Area | ChartKind::Scatter => {
                    let centers =
                        points.iter().map(|rect| (rect.x + 4.0, rect.y + 4.0)).collect::<Vec<_>>();
                    if self.props.kind != ChartKind::Scatter && !centers.is_empty() {
                        let mut path = Path::new().move_to(centers[0].0, centers[0].1);
                        for (x, y) in &centers[1..] {
                            path = path.line_to(*x, *y);
                        }
                        if self.props.kind == ChartKind::Area {
                            let bottom = plot.y + plot.height;
                            let (last_x, first_x) = (centers[centers.len() - 1].0, centers[0].0);
                            let fill = path
                                .clone()
                                .line_to(last_x, bottom)
                                .line_to(first_x, bottom)
                                .close();
                            list = list.fill_path(
                                fill,
                                Paint::color(Color::rgba(color.red, color.green, color.blue, 64)),
                            );
                        }
                        list = list.stroke_path(path, Paint::color(color).stroke_width(2.0));
                    }
                    for rect in points {
                        list = list.fill_ellipse(rect.rect(), Paint::color(color));
                    }
                }
                ChartKind::Pie => {}
            }
        }
        self.draw_focus(list)
    }

    fn draw_pie(&self, mut list: DrawList) -> DrawList {
        let plot = self.plot();
        let radius = plot.width.min(plot.height) / 2.0;
        let (cx, cy) = (plot.x + plot.width / 2.0, plot.y + plot.height / 2.0);
        let values =
            self.props.series.first().map(|series| series.values.clone()).unwrap_or_default();
        let total: f64 = values.iter().map(|value| value.max(0.0)).sum();
        let mut angle = -std::f64::consts::FRAC_PI_2;
        for (index, value) in values.iter().enumerate() {
            let sweep =
                if total > 0.0 { value.max(0.0) / total * std::f64::consts::TAU } else { 0.0 };
            let mut path = Path::new().move_to(cx, cy);
            let steps = ((sweep / 0.1).ceil().max(1.0)) as usize;
            for step in 0..=steps {
                let at = angle + sweep * step as f64 / steps as f64;
                path = path.line_to(cx + radius * at.cos() as f32, cy + radius * at.sin() as f32);
            }
            list = list.fill_path(path.close(), Paint::color(PALETTE[index % PALETTE.len()]));
            angle += sweep;
        }
        self.draw_focus(list)
    }

    fn draw_focus(&self, list: DrawList) -> DrawList {
        match self.focused {
            Some((series, index)) if self.props.kind != ChartKind::Pie => {
                let rect = self.point(series, index);
                let ring =
                    RectF::new(rect.x - 3.0, rect.y - 3.0, rect.width + 6.0, rect.height + 6.0);
                list.stroke_rect(ring, Paint::color(Color::rgb(0, 0, 0)).stroke_width(2.0))
            }
            _ => list,
        }
    }

    fn announcement(&self, series: usize, index: usize) -> String {
        let name = &self.props.series[series].name;
        let category = self.props.categories.get(index).map_or("", String::as_str);
        let value = self.props.series[series].values.get(index).copied().unwrap_or(0.0);
        format!("{name}, {category}: {}", number(value))
    }

    /// A one-sentence summary: what a screen reader reads first.
    #[must_use]
    pub fn summary(&self) -> String {
        let kind = match self.props.kind {
            ChartKind::Line => "Line chart",
            ChartKind::Area => "Area chart",
            ChartKind::Bar => "Bar chart",
            ChartKind::Scatter => "Scatter chart",
            ChartKind::Pie => "Pie chart",
        };
        let parts = self
            .props
            .series
            .iter()
            .map(|series| {
                let (low, high) =
                    series.values.iter().fold((f64::MAX, f64::MIN), |(low, high), value| {
                        (low.min(*value), high.max(*value))
                    });
                format!("{} from {} to {}", series.name, number(low), number(high))
            })
            .collect::<Vec<_>>();
        format!(
            "{kind} of {} series over {} categories: {}.",
            self.props.series.len(),
            self.props.categories.len(),
            parts.join("; ")
        )
    }

    fn accessibility(&self) -> AccessibilityInfo {
        let mut info = AccessibilityInfo::new(AccessibilityRole::Table)
            .name(self.props.title.clone())
            .description(self.summary())
            .focusable(true);
        for (series, data) in self.props.series.iter().enumerate() {
            for index in 0..data.values.len() {
                let rect = if self.props.kind == ChartKind::Pie {
                    self.plot()
                } else {
                    self.point(series, index)
                };
                let bounds = Rect::new(
                    rect.x as i32,
                    rect.y as i32,
                    rect.width.max(1.0) as i32,
                    rect.height.max(1.0) as i32,
                );
                let cell = AccessibilityInfo::new(AccessibilityRole::Cell)
                    .name(self.announcement(series, index))
                    .selected(self.focused == Some((series, index)))
                    .position_in_set(
                        u32::try_from(index + 1).unwrap_or(0),
                        u32::try_from(data.values.len()).unwrap_or(0),
                    );
                info = info.element(VirtualElement::new(
                    format!("point-{series}-{index}"),
                    cell,
                    bounds,
                ));
            }
        }
        info
    }
}

impl Component for Chart {
    type Props = ChartProps;
    type Message = ();

    fn new(props: ChartProps) -> Self {
        Self { props, focused: None }
    }
    fn props(&self) -> &ChartProps {
        &self.props
    }
    fn set_props(&mut self, props: ChartProps) {
        self.props = props;
        self.focused = None;
    }

    fn view(&self) -> Node {
        let (width, height) = self.size();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "pixel sizes")]
        let layout = LayoutStyle::new()
            .width(SizeMode::Fixed(width as i32))
            .height(SizeMode::Fixed(height as i32));
        let focused = self.focused.map_or_else(
            || "Focus the chart and use the arrow keys to read its points.".to_owned(),
            |(series, index)| self.announcement(series, index),
        );
        Node::column(
            "chart",
            [
                Node::label("title", self.props.title.clone()),
                Node::canvas("plot", self.draw(), layout).with_accessibility(self.accessibility()),
                Node::label("point", focused).with_accessibility(
                    AccessibilityInfo::new(AccessibilityRole::Status).live(LiveRegion::Polite),
                ),
            ],
        )
    }

    fn update(&mut self, event: Event) {
        let Event::KeyDown { key, .. } = event else { return };
        let series_count = self.props.series.len();
        if series_count == 0 {
            return;
        }
        let (series, index) = self.focused.unwrap_or((0, 0));
        let length = self.props.series[series].values.len();
        self.focused = Some(match key {
            KeyCode::ArrowRight => (series, (index + 1).min(length.saturating_sub(1))),
            KeyCode::ArrowLeft => (series, index.saturating_sub(1)),
            KeyCode::ArrowDown => ((series + 1) % series_count, index),
            KeyCode::ArrowUp => ((series + series_count - 1) % series_count, index),
            KeyCode::Home => (series, 0),
            KeyCode::End => (series, length.saturating_sub(1)),
            _ => return,
        });
    }
}
