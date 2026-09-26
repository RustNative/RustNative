//! The same application at both ends of the range: a desktop window and a
//! device's 240×320 screen, with the same behaviour and a layout that fits
//! each.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use rustnative_core::{Component, Size, Window};
use rustnative_headless::{HeadlessApp, Query};
use span::{DESKTOP_WINDOW, DEVICE_SCREEN, Thermostat};

fn run(screen: Size) -> HeadlessApp {
    HeadlessApp::launch(Window::new("thermostat", screen), || Thermostat::new(()))
}

#[test]
fn it_behaves_the_same_on_a_desktop_and_a_device() {
    for screen in [DESKTOP_WINDOW, DEVICE_SCREEN] {
        let mut app = run(screen);
        for _ in 0..3 {
            app.click(&Query::key("warmer")).unwrap();
        }
        app.click(&Query::key("mode")).unwrap();
        assert_eq!(app.find(&Query::key("target")).unwrap().text.as_deref(), Some("23 °C"));
        assert_eq!(app.find(&Query::key("mode")).unwrap().text.as_deref(), Some("Off"));
        // Everything is on the screen it runs on.
        for key in ["target", "cooler", "warmer", "mode"] {
            let rect = app.find(&Query::key(key)).unwrap().window_rect;
            assert!(
                rect.x + rect.width <= i32::try_from(screen.width).unwrap()
                    && rect.y + rect.height <= i32::try_from(screen.height).unwrap(),
                "{key} fits {screen:?}: {rect:?}"
            );
        }
    }
}
