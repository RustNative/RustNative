//! Virtual lists on GTK (Milestone 28's claims, Phase 5): a hundred
//! thousand items realize a screenful of widgets, scrolling inside a range
//! renders nothing, crossing one renders once, rows recycle their widgets,
//! insertion above the viewport leaves the visible rows still, and
//! estimated items are measured.
//!
//! Scrolling is the list's `GtkScrolledWindow` adjustment moving — where a
//! wheel, a scrollbar drag, a keyboard scroll, or an assistive technology's
//! scroll all end up.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use rustnative_core::{
    Application, ColumnStyle, Component, ComponentContext, Event, ItemExtent, LayoutStyle, Node,
    NodeId, Size, SizeMode, VirtualListStyle, VirtualRange, Window, WindowId,
};

use super::testing::{Harness, on_gtk};

const ROW: u32 = 20;
const OVERSCAN: usize = 1;

#[derive(Clone, PartialEq)]
struct Props {
    renders: Rc<Cell<u32>>,
    ranges: Rc<RefCell<Vec<VirtualRange>>>,
}

/// A list of `count` items, of which only `range` is ever rendered.
struct Rows {
    props: Props,
    count: usize,
    range: VirtualRange,
    inserted_above: usize,
    estimated: bool,
    row_height: i32,
}

impl Rows {
    fn datum(&self, index: usize) -> String {
        match index.checked_sub(self.inserted_above) {
            Some(datum) => datum.to_string(),
            None => format!("-{}", self.inserted_above - index),
        }
    }
}

impl Component for Rows {
    type Props = Props;
    type Message = ();

    fn new(props: Props) -> Self {
        Self {
            props,
            count: 100_000,
            range: VirtualRange::EMPTY,
            inserted_above: 0,
            estimated: false,
            row_height: 20,
        }
    }
    fn props(&self) -> &Props {
        &self.props
    }
    fn set_props(&mut self, props: Props) {
        self.props = props;
    }

    fn view(&self) -> Node {
        let extent =
            if self.estimated { ItemExtent::Estimated(ROW) } else { ItemExtent::Fixed(ROW) };
        let rows = self.range.indices().map(|index| {
            Node::column_with_layout(
                format!("row-{}", self.datum(index)),
                [],
                LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(self.row_height)),
                ColumnStyle::new(),
            )
            .with_item_index(index)
        });
        Node::column(
            "root",
            [
                Node::button_with_layout(
                    "insert",
                    "Insert above",
                    LayoutStyle::new().height(SizeMode::Fixed(24)),
                ),
                Node::button_with_layout(
                    "measure",
                    "Estimate",
                    LayoutStyle::new().height(SizeMode::Fixed(24)),
                ),
                Node::virtual_list_with_layout(
                    "list",
                    VirtualListStyle::new(self.count, extent).overscan(OVERSCAN),
                    LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(210)),
                    rows,
                ),
            ],
        )
    }

    fn render(&mut self, _context: &mut ComponentContext<'_, ()>) -> Node {
        self.props.renders.set(self.props.renders.get() + 1);
        self.view()
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::VisibleRangeChanged { range, .. } => {
                self.props.ranges.borrow_mut().push(range);
                self.range = range;
            }
            Event::Click { target } if target == NodeId::from_key("insert") => {
                self.inserted_above += 10;
                self.count += 10;
            }
            Event::Click { target } if target == NodeId::from_key("measure") => {
                self.estimated = true;
                self.row_height = 50;
            }
            _ => {}
        }
    }
}

struct Fixture {
    renders: Rc<Cell<u32>>,
    ranges: Rc<RefCell<Vec<VirtualRange>>>,
}

impl Fixture {
    fn new() -> Self {
        Self { renders: Rc::new(Cell::new(0)), ranges: Rc::new(RefCell::new(Vec::new())) }
    }

    fn application(&self) -> Application {
        Application::new(
            Rows::new(Props { renders: self.renders.clone(), ranges: self.ranges.clone() }),
            Window::new("virtual list", Size::new(320, 400)),
        )
    }

    fn last_range(&self) -> VirtualRange {
        self.ranges.borrow().last().copied().expect("a range was reported")
    }
}

fn list() -> NodeId {
    NodeId::from_key("list")
}

/// Each realized row's item index and widget.
fn rows(harness: &Harness) -> Vec<(usize, gtk::Widget)> {
    harness.with_registry(|registry| {
        let runtime = &registry.windows[&WindowId::PRIMARY];
        runtime
            .renderer
            .snapshot
            .children_of(list())
            .map(|child| {
                let index = child.item_index.expect("every row declares its item index");
                (index, runtime.renderer.widget(child.id).expect("realized").clone())
            })
            .collect()
    })
}

/// Scrolls the list to `offset` pixels, as anything that scrolls it does.
fn scroll_to(harness: &Harness, offset: f64) {
    let scrolled = harness.expect_as::<gtk::ScrolledWindow>("list");
    scrolled.vadjustment().set_value(offset);
    harness.pump();
    // GTK moves the content to the new offset at its next layout.
    super::testing::wait_frames(&scrolled, 2);
    harness.pump();
}

#[test]
fn a_hundred_thousand_items_realize_only_a_screenful_of_widgets() {
    on_gtk(|| {
        let fixture = Fixture::new();
        let mut application = fixture.application();
        // SAFETY: `application` outlives `harness`, declared after it.
        let harness = unsafe { Harness::attach(&mut application) };
        assert_eq!(fixture.last_range(), VirtualRange { first: 0, last_exclusive: 12 });
        assert_eq!(rows(&harness).len(), 12, "12 widgets for 100,000 items, and only those");
        // The scrollable length is the whole list's.
        let scrolled = harness.expect_as::<gtk::ScrolledWindow>("list");
        assert!(
            scrolled.vadjustment().upper() >= f64::from(100_000 * ROW),
            "{}",
            scrolled.vadjustment().upper()
        );
    });
}

#[test]
fn scrolling_inside_a_range_never_renders_and_crossing_one_renders_once() {
    on_gtk(|| {
        let fixture = Fixture::new();
        let mut application = fixture.application();
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        let settled = fixture.renders.get();
        let first = fixture.last_range();
        scroll_to(&harness, 5.0);
        assert_eq!(fixture.renders.get(), settled, "a scroll inside the range must not rerender");
        assert_eq!(fixture.last_range(), first);
        scroll_to(&harness, 45.0);
        assert_eq!(fixture.renders.get(), settled + 1, "crossing a boundary renders exactly once");
        assert!(fixture.last_range().first > first.first, "and it moved down the list");
    });
}

#[test]
fn rows_recycle_their_widgets_as_the_range_moves() {
    on_gtk(|| {
        let fixture = Fixture::new();
        let mut application = fixture.application();
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        let before = rows(&harness);
        scroll_to(&harness, 40.0);
        let after = rows(&harness);
        assert_ne!(
            after.first().map(|(index, _)| *index),
            before.first().map(|(index, _)| *index),
            "the range moved"
        );
        let reused =
            after.iter().filter(|(_, widget)| before.iter().any(|(_, old)| old == widget)).count();
        let growth = after.len().saturating_sub(before.len());
        assert_eq!(after.len() - reused, growth, "only growth creates widgets");
        for (index, widget) in &after {
            if let Some((_, previous)) = before.iter().find(|(old, _)| old == index) {
                assert_eq!(widget, previous, "row {index} stayed realized and kept its widget");
            }
        }
    });
}

#[test]
fn inserting_items_above_the_viewport_leaves_the_visible_rows_stationary() {
    on_gtk(|| {
        let fixture = Fixture::new();
        let mut application = fixture.application();
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        scroll_to(&harness, 200.0);
        let window = harness.gtk_window(WindowId::PRIMARY).expect("a window");
        let first = fixture.last_range().first;
        // The datum, by its row's key: a recycled widget may realize another
        // row after the render, so the widget is looked up again by node.
        let key = harness.with_registry(|registry| {
            let snapshot = &registry.windows[&WindowId::PRIMARY].renderer.snapshot;
            snapshot
                .children_of(list())
                .find(|child| child.item_index.is_some_and(|index| index > first + 2))
                .map(|child| child.id)
                .expect("a row in view")
        });
        let position = || {
            let widget = harness.with_registry(|registry| {
                registry.windows[&WindowId::PRIMARY].renderer.widget(key).cloned()
            });
            widget.expect("still realized").compute_bounds(&window).expect("placed").y()
        };
        let before = position();
        harness.click("insert");
        // GTK moves the scrolled content to the new offset at its next frame.
        super::testing::wait_frames(&window, 2);
        let after = position();
        assert!((after - before).abs() < 0.5, "the row moved from {before} to {after}");
    });
}

#[test]
fn estimated_items_are_measured_and_move_the_offsets_after_them() {
    on_gtk(|| {
        let fixture = Fixture::new();
        let mut application = fixture.application();
        // SAFETY: as above.
        let harness = unsafe { Harness::attach(&mut application) };
        harness.click("measure");
        let (extent, next) = harness.with_registry(|registry| {
            let extents =
                &registry.windows[&WindowId::PRIMARY].renderer.virtual_lists.extents()[&list()];
            (extents.extent_of(0), extents.offset_of(1))
        });
        assert_eq!((extent, next), (50, 50), "measured at 50 px, and the next row starts there");
    });
}
