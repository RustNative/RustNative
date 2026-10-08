//! Animation, virtual lists, canvas, native surfaces, and host content on a
//! real device (Phase 5).

use std::time::Duration;

use rustnative_core::{
    AnimatedProperty, Application, Color, ColumnStyle, Component, ComponentContext, DrawList,
    Event, HostContent, ItemExtent, LayoutStyle, Node, Paint, RectF, Size, SizeMode, Transition,
    VirtualListStyle, VirtualRange, Window, WindowId,
};

use raw_window_handle::HasWindowHandle as _;

use super::harness::{Harness, Instrumentation, java, keep, on_main, with_kept};
use crate::jni_host::{Arg, Class};

/// A box whose opacity transitions when clicked.
struct Fader {
    faded: bool,
}

impl Component for Fader {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { faded: false }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        Node::column(
            "root",
            [
                Node::button("fade", "Fade"),
                Node::label("box", "Fading")
                    .with_opacity(if self.faded { 0.0 } else { 1.0 })
                    .with_transition(
                        AnimatedProperty::Opacity,
                        Transition::new(Duration::from_millis(600)),
                    ),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if matches!(event, Event::Click { .. }) {
            self.faded = true;
        }
    }
}

fn alpha(key: &'static str) -> f32 {
    on_main(move || {
        with_kept(|harness| {
            let view = harness.expect(key);
            java(Class::Probe, "style", "(Landroid/view/View;)[F", &[Arg::Obj(&view)]).floats()[6]
        })
    })
}

pub(super) fn a_transition_runs_across_frames_and_settles(instrumentation: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(Application::new(
            Fader::new(()),
            Window::new("Fader", Size::new(300, 300)),
        ));
        harness.click("fade");
        keep(harness);
    });
    // The main looper runs Choreographer's frames while this thread waits.
    std::thread::sleep(Duration::from_millis(250));
    let midway = alpha("box");
    assert!(midway > 0.02 && midway < 0.98, "partway through the transition: {midway}");
    // Eased, the value nears its end before the duration does: wait for the
    // timeline, then read where it settled.
    instrumentation.wait_for("the transition to end", Duration::from_secs(5), || {
        on_main(|| {
            with_kept(|harness| {
                harness.with_registry(|registry| {
                    !registry.windows[&WindowId::PRIMARY].animation.timeline.is_active()
                })
            })
        })
    });
    assert!(alpha("box") < 0.01, "it settled at its target");
}

/// A hundred thousand rows, of which only the visible range is rendered.
struct Rows {
    range: VirtualRange,
    renders: u32,
}

impl Component for Rows {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { range: VirtualRange::EMPTY, renders: 0 }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let rows = self.range.indices().map(|index| {
            Node::column_with_layout(
                format!("row-{index}"),
                [Node::label(format!("label-{index}"), format!("Row {index}"))],
                LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(40)),
                ColumnStyle::new(),
            )
            .with_item_index(index)
        });
        Node::virtual_list_with_layout(
            "list",
            VirtualListStyle::new(100_000, ItemExtent::Fixed(40)).overscan(2),
            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(400)),
            rows,
        )
    }
    fn render(&mut self, _context: &mut ComponentContext<'_, ()>) -> Node {
        self.renders += 1;
        self.view()
    }
    fn update(&mut self, event: Event) {
        if let Event::VisibleRangeChanged { range, .. } = event {
            self.range = range;
        }
    }
}

pub(super) fn a_hundred_thousand_rows_realize_a_screenful_and_recycle(_: &Instrumentation) {
    on_main(|| {
        let harness = Harness::launch(Application::new(
            Rows::new(()),
            Window::new("Rows", Size::new(360, 600)),
        ));
        let realized = harness.realized();
        assert!(realized < 60, "a screenful of rows, not a hundred thousand ({realized} views)");
        assert!(harness.view("label-0").is_some(), "the first row is realized");
        let (created, _) = harness.census();
        let list = harness.expect("list");
        let density = harness.density();
        // Far down the list: rows recycle their views.
        java(
            Class::Views,
            "scrollTo",
            "(Landroid/view/View;II)V",
            &[Arg::Obj(&list), Arg::Int(0), Arg::Int(crate::units::to_px(40 * 50_000, density))],
        );
        harness.pump();
        assert!(harness.view("label-50000").is_some(), "row 50 000 is realized after the scroll");
        assert!(harness.view("label-0").is_none(), "row 0 is gone");
        let (after, _) = harness.census();
        assert!(
            after - created < 40,
            "the new rows reused the old rows' views ({} made)",
            after - created
        );
        assert!(harness.realized() < 60);
    });
}

/// A canvas of known colours.
struct Picture;

impl Component for Picture {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let list = DrawList::new()
            .fill_rect(RectF::new(0.0, 0.0, 50.0, 50.0), Paint::color(Color::rgb(255, 0, 0)))
            .push_clip(RectF::new(50.0, 0.0, 25.0, 50.0))
            .fill_rect(RectF::new(50.0, 0.0, 50.0, 50.0), Paint::color(Color::rgb(0, 0, 255)))
            .push(rustnative_core::DrawCommand::Pop);
        Node::canvas(
            "picture",
            list,
            LayoutStyle::new().width(SizeMode::Fixed(100)).height(SizeMode::Fixed(50)),
        )
    }
    fn update(&mut self, _event: Event) {}
}

pub(super) fn a_canvas_draws_its_list(_: &Instrumentation) {
    on_main(|| {
        let harness =
            Harness::launch(Application::new(Picture, Window::new("Picture", Size::new(300, 200))));
        let view = harness.expect("picture");
        let density = harness.density();
        let (width, height) = (crate::units::to_px(100, density), crate::units::to_px(50, density));
        let pixels = java(
            Class::Canvas,
            "render",
            "(Landroid/view/View;II)[I",
            &[Arg::Obj(&view), Arg::Int(width), Arg::Int(height)],
        )
        .ints();
        let at = |x: i32, y: i32| {
            u32::from_ne_bytes(pixels[usize::try_from(y * width + x).unwrap_or(0)].to_ne_bytes())
        };
        let mid_y = height / 2;
        assert_eq!(at(crate::units::to_px(25, density), mid_y), 0xffff_0000, "the red square");
        assert_eq!(
            at(crate::units::to_px(60, density), mid_y),
            0xff00_00ff,
            "the blue square, inside the clip"
        );
        assert_eq!(
            at(crate::units::to_px(90, density), mid_y) >> 24,
            0,
            "outside the clip, nothing is drawn"
        );
    });
}

/// A native surface.
struct Surfaced {
    reported: Option<(rustnative_core::SurfaceId, Size)>,
}

impl Component for Surfaced {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self { reported: None }
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let text = self.reported.map_or_else(
            || "none".to_owned(),
            |(id, size)| format!("{} {}x{}", id.raw(), size.width, size.height),
        );
        Node::column(
            "root",
            [
                Node::native_surface(
                    "surface",
                    LayoutStyle::new().width(SizeMode::Fixed(120)).height(SizeMode::Fixed(80)),
                ),
                Node::label("reported", text),
            ],
        )
    }
    fn update(&mut self, event: Event) {
        if let Event::SurfaceResized { surface, size, .. } = event {
            self.reported = Some((surface, size));
        }
    }
}

pub(super) fn a_native_surface_is_a_live_window_the_size_of_its_node(
    instrumentation: &Instrumentation,
) {
    on_main(|| {
        keep(Harness::launch(Application::new(
            Surfaced::new(()),
            Window::new("Surface", Size::new(300, 300)),
        )));
    });
    instrumentation.wait_for("the surface", Duration::from_secs(10), || {
        on_main(|| with_kept(|harness| harness.text("reported").is_some_and(|text| text != "none")))
    });
    let (raw, size, frame) = on_main(|| {
        with_kept(|harness| {
            let text = harness.text("reported").unwrap_or_default();
            let (raw, size) = text.split_once(' ').unwrap_or_default();
            (raw.parse::<u64>().unwrap_or(0), size.to_owned(), harness.frame("surface"))
        })
    });
    // Its edges are rounded to pixels where it sits, so the size is its
    // view's, which may differ from 120 dp × density by a pixel.
    assert_eq!(
        size,
        format!("{}x{}", frame[2], frame[3]),
        "the surface is its node's size in device pixels"
    );
    let handle = crate::native_surface(rustnative_core::SurfaceId::from_raw(raw))
        .expect("a live surface has a handle");
    assert!(handle.window_handle().is_ok(), "the handle is live");
    on_main(super::harness::stop_running);
    instrumentation.wait_idle();
    assert!(handle.window_handle().is_err(), "a released surface's handle reports it");
}

/// A web page.
struct Page;

impl Component for Page {
    type Props = ();
    type Message = ();
    fn new((): ()) -> Self {
        Self
    }
    fn props(&self) -> &() {
        &()
    }
    fn set_props(&mut self, (): ()) {}
    fn view(&self) -> Node {
        let content =
            HostContent::Web { url: "data:text/html,<title>Rust%20Native</title><p>Hello".into() };
        Node::foreign(
            "page",
            content.kind(),
            LayoutStyle::new().width(SizeMode::Fill).height(SizeMode::Fixed(200)),
        )
    }
    fn update(&mut self, _event: Event) {}
}

pub(super) fn web_content_loads_in_a_web_view(instrumentation: &Instrumentation) {
    on_main(|| {
        keep(Harness::launch(Application::new(Page, Window::new("Page", Size::new(360, 400)))));
    });
    assert_eq!(on_main(|| with_kept(|harness| harness.class_of("page"))), "android.webkit.WebView");
    instrumentation.wait_for("the page's title", Duration::from_secs(20), || {
        on_main(|| {
            with_kept(|harness| {
                java(
                    Class::Host,
                    "webTitle",
                    "(Landroid/view/View;)Ljava/lang/String;",
                    &[Arg::Obj(&harness.expect("page"))],
                )
                .string()
                .as_deref()
                    == Some("Rust Native")
            })
        })
    });
}
