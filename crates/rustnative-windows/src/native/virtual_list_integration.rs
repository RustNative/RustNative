//! Milestone 28 native integration tests: virtualization against real
//! windows.
//!
//! Every assertion here is about what Windows actually did — how many
//! controls exist, which `HWND` each row is on, where `GetWindowRect` says
//! a row is — driven through the production message loop. Scrolling is a
//! real `WM_MOUSEWHEEL`, not a call into the renderer, so what is tested is
//! the path a person's wheel takes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rustnative_core::{
    Application, ColumnStyle, Component, ComponentContext, Event, ItemExtent, LayoutStyle, Node,
    NodeId, Size, SizeMode, VirtualListStyle, VirtualRange, Window, WindowId,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, WM_MOUSEWHEEL};

use super::harness::NativeHarness;

/// 20 px rows, one row of overscan on each side: a range small enough to
/// state exactly in an assertion.
const ROW: u32 = 20;
const OVERSCAN: usize = 1;

#[derive(Clone, PartialEq)]
struct Props {
    renders: Rc<Cell<u32>>,
    ranges: Rc<RefCell<Vec<VirtualRange>>>,
}

/// A list of `count` items, of which only `range` is ever rendered.
///
/// `inserted_above` stands in for data arriving at the top of the list: it
/// shifts every datum one index later without changing which *datum* a row
/// key names, which is exactly the situation scroll anchoring exists for.
struct Rows {
    props: Props,
    count: usize,
    range: VirtualRange,
    inserted_above: usize,
    estimated: bool,
    /// How tall each row asks to be, for the estimated-extent test.
    row_height: i32,
}

impl Rows {
    /// The datum shown at item `index` — stable under insertion above it.
    fn datum(&self, index: usize) -> String {
        match index.checked_sub(self.inserted_above) {
            Some(datum) => datum.to_string(),
            None => format!("-{}", self.inserted_above - index),
        }
    }

    fn key(&self, index: usize) -> String {
        format!("row-{}", self.datum(index))
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
                self.key(index),
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
                // 210 px: deliberately not a multiple of the row height, so
                // a scroll of a few pixels provably stays inside one range.
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
            // Ten data items arrive at the top of the list.
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

/// The list's own node.
fn list() -> NodeId {
    NodeId::from_key("list")
}

/// How many rows are realized natively right now.
fn realized(harness: &NativeHarness) -> usize {
    harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime.renderer.snapshot.children_of(list()).count()
    })
}

/// Each realized row's key and the `HWND` realizing it.
fn rows(harness: &NativeHarness) -> Vec<(usize, HWND)> {
    harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime
            .renderer
            .snapshot
            .children_of(list())
            .map(|child| {
                let index = child.item_index.expect("every row declares its item index");
                let hwnd = runtime
                    .renderer
                    .registry
                    .get(child.id)
                    .expect("every realized row has a native window")
                    .hwnd();
                (index, hwnd)
            })
            .collect::<Vec<_>>()
    })
}

fn rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live control; `rect` is exclusively borrowed.
    let read = unsafe { GetWindowRect(hwnd, &raw mut rect) } != 0;
    assert!(read, "GetWindowRect on a live control must succeed");
    rect
}

/// Scrolls the list with a real wheel message, aimed at a point inside the
/// list's own viewport.
///
/// `notches` is in wheel detents, one message each; positive scrolls down.
/// The backend divides a detent's `WHEEL_DELTA` by three, so one detent
/// moves the list 40 px — two rows.
fn wheel(harness: &mut NativeHarness, notches: i32) {
    let viewport = rect(harness.expect_control(WindowId::PRIMARY, "list"));
    let (x, y) = (viewport.left + 5, viewport.top + 5);
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "the documented WM_MOUSEWHEEL layout: two 16-bit coordinates in one LPARAM"
    )]
    let lparam = ((((y as u32) & 0xFFFF) << 16) | ((x as u32) & 0xFFFF)) as isize;
    // A scroll *down* is a negative `WHEEL_DELTA`, carried as a signed
    // 16-bit value in the high word of `wParam`.
    // -120 as a 16-bit two's-complement value (`i16::cast_unsigned` is
    // newer than this crate's MSRV).
    let wparam = (u32::from(0xFF88u16) << 16) as usize;
    let window = harness.hwnd(WindowId::PRIMARY);
    for _ in 0..notches {
        // Posted, not sent: wheel routing happens in the loop's
        // pre-dispatch pass, which a sent message would skip.
        harness.post(window, WM_MOUSEWHEEL, wparam, lparam);
    }
}

/// A list of a hundred thousand items realizes a screenful of windows, and
/// the items it realizes are the ones the viewport is over.
///
/// This is the milestone's whole claim, stated as a test: without
/// virtualization this creates 100,000 `HWND`s and exhausts the desktop's
/// USER handle quota long before it finishes.
#[test]
fn a_hundred_thousand_items_realize_only_a_screenful_of_windows() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    // SAFETY: `application` is declared first, so it outlives the harness.
    let harness = unsafe { NativeHarness::attach(&mut application) };

    // 210 px over 20 px rows is rows 0..=10 in view, plus one of overscan
    // below (none above: the list is at the top).
    assert_eq!(fixture.last_range(), VirtualRange { first: 0, last_exclusive: 12 });
    assert_eq!(realized(&harness), 12, "12 native rows for 100,000 items, and only those");
}

/// Scrolling inside the realized range reaches no component; scrolling out
/// of it reaches it exactly once.
#[test]
fn scrolling_inside_a_range_never_renders_and_crossing_one_renders_once() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    let settled = fixture.renders.get();
    let first_range = fixture.last_range();

    // Five pixels: the same rows still cover the viewport.
    harness.with_runtime_mut(WindowId::PRIMARY, |runtime| {
        runtime.renderer.scroll_container(list(), 0, 5);
        super::virtual_list::after_render(runtime);
    });
    assert_eq!(fixture.renders.get(), settled, "a scroll inside the range must not rerender");
    assert_eq!(fixture.last_range(), first_range);

    // One wheel notch: 40 px, two rows, so the range has to move.
    wheel(&mut harness, 1);
    assert_eq!(fixture.renders.get(), settled + 1, "crossing a boundary renders exactly once");
    assert!(fixture.last_range().first > first_range.first, "and it moved down the list");
}

/// The windows a scroll frees are the windows the newly visible rows are
/// realized on.
#[test]
fn rows_recycle_their_native_windows_as_the_range_moves() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let before = rows(&harness);
    let before_windows = before.iter().map(|(_, hwnd)| *hwnd).collect::<Vec<_>>();

    wheel(&mut harness, 1);
    let after = rows(&harness);
    assert_ne!(
        after.first().map(|(index, _)| *index),
        before.first().map(|(index, _)| *index),
        "the test is pointless unless the range actually moved"
    );

    // Only rows beyond what the old range held may need a new window; every
    // other row must be on one that already existed.
    let reused = after.iter().filter(|(_, hwnd)| before_windows.contains(hwnd)).count();
    let growth = after.len().saturating_sub(before.len());
    assert_eq!(
        after.len() - reused,
        growth,
        "{} windows created for a range that grew by {growth}",
        after.len() - reused
    );
    assert!(
        before.iter().any(|(index, hwnd)| {
            !after.iter().any(|(after_index, _)| after_index == index)
                && after.iter().any(|(_, after_hwnd)| after_hwnd == hwnd)
        }),
        "a row that scrolled out must have handed its window to a row that scrolled in"
    );

    // The rows still in range kept their own window, not just any window.
    for (index, hwnd) in &after {
        if let Some((_, previous)) = before.iter().find(|(before_index, _)| before_index == index) {
            assert_eq!(
                hwnd, previous,
                "row {index} was realized, stayed realized, and moved window"
            );
        }
    }
}

/// Inserting items above the viewport leaves what a person is looking at
/// exactly where it was.
#[test]
fn inserting_items_above_the_viewport_leaves_the_visible_rows_stationary() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    // Scroll well into the list, then note where one specific datum is.
    wheel(&mut harness, 5);
    let anchored = rows(&harness)
        .into_iter()
        .find(|(index, _)| *index > fixture.last_range().first)
        .expect("a row below the first");
    let (index, hwnd) = anchored;
    let before = rect(hwnd).top;
    let key = harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime.renderer.registry.id_for_hwnd(hwnd).expect("a registered row")
    });

    harness.click(WindowId::PRIMARY, "insert");

    let moved = harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime.renderer.registry.get(key).map(super::registry::NativeObject::hwnd)
    });
    let moved = moved.expect("the anchored datum is still realized");
    assert_eq!(rect(moved).top, before, "the datum from item {index} must not move on screen");
}

/// A list whose items are estimated measures the ones it realizes, and the
/// offsets of every later item move with what it learned.
#[test]
fn estimated_items_are_measured_and_move_the_offsets_after_them() {
    let fixture = Fixture::new();
    let mut application = fixture.application();
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };

    let estimated_total = harness.with_runtime(WindowId::PRIMARY, |runtime| {
        runtime.renderer.virtual_lists.extents()[&list()].total()
    });
    assert_eq!(estimated_total, 100_000 * ROW, "fixed rows: the list is exactly as long as stated");

    // Switch the list to estimated 20 px rows that are really 50 px.
    harness.click(WindowId::PRIMARY, "measure");

    let offsets = harness.with_runtime(WindowId::PRIMARY, |runtime| {
        let extents = &runtime.renderer.virtual_lists.extents()[&list()];
        (extents.extent_of(0), extents.offset_of(1))
    });
    assert_eq!(offsets.0, 50, "the row measured 50 px, not the 20 px it was estimated at");
    assert_eq!(offsets.1, 50, "and the row after it starts where it actually ends");
}
