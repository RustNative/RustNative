//! The web loading path in a real browser (`C42`): images served as
//! responsive WebP with their size reserved, the first fetched first and
//! the rest lazily, and no layout shift; a font subset, preloaded, and
//! used; links prefetched on hover, but not when the person saves data.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unused_async,
    missing_docs,
    reason = "tests"
)]

use std::time::Duration;

use rustnative_core::{Component, ComponentContext, Event, ImageData, Node};
use rustnative_server::{ServerApp, get};
use rustnative_web::form::link;
use rustnative_web::{Client, Head, Page};
use rustnative_web_testing::browser_or_skip;

#[rustnative_web::client]
pub mod nothing {
    use rustnative_core::{Event, Node};
    use rustnative_web::Effects;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct Nothing {
        pub on: bool,
    }

    impl Nothing {
        pub fn update(&mut self, event: Event, fx: &mut Effects<()>) {
            let _ = (event, fx);
        }

        #[must_use]
        pub fn view(&self) -> Node {
            Node::label("island", "An island, so the runtime is here")
        }
    }
}

fn picture(width: u32, height: u32, shade: u8) -> ImageData {
    let pixels: Vec<u8> = (0..width * height)
        .flat_map(|index| [shade, u8::try_from(index % 251).unwrap(), 90, 255])
        .collect();
    ImageData::rgba(width, height, pixels, false).unwrap()
}

struct Gallery;

impl Component for Gallery {
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
        Node::column("gallery", [])
    }
    fn update(&mut self, _: Event) {}
    fn render(&mut self, context: &mut ComponentContext<'_, ()>) -> Node {
        let island = context.child_with_props::<Client<nothing::Nothing>, _>(
            "nothing",
            nothing::Nothing::default(),
            Client::new,
        );
        Node::column(
            "gallery",
            [
                Node::label("title", "Gallery"),
                Node::image("hero", picture(800, 400, 30)),
                island,
                link(context, "next", "Next page", "/next"),
                Node::label_with_layout(
                    "spacer",
                    "",
                    rustnative_core::LayoutStyle::new()
                        .height(rustnative_core::SizeMode::Fixed(3000)),
                ),
                Node::image("below", picture(640, 320, 200)),
            ],
        )
    }
}

struct Next;

impl Component for Next {
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
        Node::label("next", "The next page")
    }
    fn update(&mut self, _: Event) {}
}

fn serve() -> String {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let head =
        |title: &str| Head::new(title, "A page served to a real browser by the loading tests.");
    let mut app = ServerApp::new()
        .route(
            "/",
            get(move || async move { Page::new::<Gallery>(head("Gallery"), ()).lang("en") })
                .public(),
        )
        .route(
            "/next",
            get(move || async move { Page::new::<Next>(head("Next"), ()).lang("en") }).public(),
        );
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/arial.ttf") {
        let font = rustnative_web::fonts::Font::new("Brand", 400, bytes).unwrap();
        app = app.font(&font, "").unwrap();
    }
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        runtime.block_on(app.into_service().serve(listener, std::future::pending()))
    });
    format!("http://localhost:{port}")
}

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn the_loading_path_in_a_browser() {
    let Some(browser) = browser_or_skip("the_loading_path_in_a_browser") else { return };
    let origin = serve();
    let page = browser.page().unwrap();
    page.eval("0").unwrap();
    page.goto(&format!("{origin}/")).unwrap();
    page.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();

    // Responsive images: WebP at several widths, size reserved, the first
    // fetched first and the one far below lazily.
    let hero = page.eval("(() => { const img = document.getElementById('hero'); return [img.getAttribute('srcset'), img.getAttribute('fetchpriority'), img.getAttribute('loading'), img.getAttribute('width'), img.naturalWidth, img.currentSrc]; })()").unwrap();
    assert!(
        hero[0].as_str().unwrap().contains(" 400w") && hero[0].as_str().unwrap().contains(" 800w"),
        "{hero}"
    );
    assert_eq!(hero[1], "high");
    assert_eq!(hero[2], serde_json::Value::Null);
    assert_eq!(hero[3], "800");
    assert!(hero[5].as_str().unwrap().contains("/_rn/img/"), "{hero}");
    page.wait_until("document.getElementById('hero').complete && document.getElementById('hero').naturalWidth > 0", WAIT).unwrap();
    assert_eq!(
        page.eval("document.getElementById('below').getAttribute('loading')").unwrap(),
        "lazy"
    );
    assert_eq!(page.eval("document.getElementById('below').complete && document.getElementById('below').naturalWidth > 0").unwrap(), false, "not fetched while far below");

    // No layout shift from framework-controlled content.
    let shift = page
        .eval("new Promise((resolve) => { let total = 0; new PerformanceObserver((list) => { for (const entry of list.getEntries()) if (!entry.hadRecentInput) total += entry.value; }).observe({ type: 'layout-shift', buffered: true }); setTimeout(() => resolve(total), 300); })")
        .unwrap();
    assert_eq!(shift.as_f64(), Some(0.0), "cumulative layout shift");
    let metrics = page
        .eval(&format!(
            "(async () => (await import('{}')).metrics())()",
            rustnative_web::runtime::runtime_url("/_rn/")
        ))
        .unwrap();
    assert!(metrics["lcp"].as_f64().is_some_and(|lcp| lcp > 0.0), "{metrics}");

    // The font: preloaded, subset, and loaded.
    if std::fs::metadata("C:/Windows/Fonts/arial.ttf").is_ok() {
        let preload = page
            .eval("document.querySelector('link[rel=preload][as=font]')?.getAttribute('href')")
            .unwrap();
        let url = preload.as_str().expect("a preloaded font");
        let size = page.eval(&format!("fetch('{url}').then((response) => response.arrayBuffer()).then((bytes) => bytes.byteLength)")).unwrap();
        let full = std::fs::metadata("C:/Windows/Fonts/arial.ttf").unwrap().len();
        assert!(size.as_u64().unwrap() * 5 < full, "{size} of {full}");
        // Nothing on the page sets the family yet: asking for it loads it.
        let loaded =
            page.eval("document.fonts.load('16px Brand').then((faces) => faces.length)").unwrap();
        assert_eq!(loaded, 1, "the subset font loads and parses");
    }

    // Prefetch (the link is in view, and then under the pointer); not
    // under Save-Data.
    let listen = "window.prefetched = []; document.addEventListener('rn:prefetch', (event) => window.prefetched.push(event.detail.url));";
    let watching = browser.page().unwrap();
    watching
        .call("Page.addScriptToEvaluateOnNewDocument", serde_json::json!({ "source": listen }))
        .unwrap();
    watching.goto(&format!("{origin}/")).unwrap();
    watching.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
    let (x, y) = watching.center("#next").unwrap();
    watching.mouse("mouseMoved", x, y, 0).unwrap();
    watching.wait_until("window.prefetched.some((url) => url.endsWith('/next'))", WAIT).unwrap();
    assert_eq!(watching.eval("window.prefetched.length").unwrap(), 1, "once");

    let saving = browser.page().unwrap();
    let source = format!(
        "Object.defineProperty(navigator, 'connection', {{ value: {{ saveData: true }} }}); {listen}"
    );
    saving
        .call("Page.addScriptToEvaluateOnNewDocument", serde_json::json!({ "source": source }))
        .unwrap();
    saving.goto(&format!("{origin}/")).unwrap();
    saving.wait_until("document.documentElement.hasAttribute('data-rn-ready')", WAIT).unwrap();
    let (x, y) = saving.center("#next").unwrap();
    saving.mouse("mouseMoved", x, y, 0).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(saving.eval("window.prefetched.length").unwrap(), 0, "no prefetch when saving data");

    // A navigation still works (from the prefetched response).
    watching.click("#next").unwrap();
    watching
        .wait_until(
            "location.pathname === '/next' && document.getElementById('next') !== null",
            WAIT,
        )
        .unwrap();
}
