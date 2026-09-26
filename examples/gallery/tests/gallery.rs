//! The gallery on the headless backend (`PLAN.md` Milestone 48's "done
//! when"): every page realizes from the library, styled by the token set,
//! and its goldens pin what each page realizes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use gallery::{Gallery, theme};
use rustnative_core::{Component, Services, Size, Window};
use rustnative_headless::{HeadlessApp, Query, assert_golden};

fn launch() -> HeadlessApp {
    HeadlessApp::launch_with(
        Window::new("Gallery", Size::new(900, 1400)),
        Services::default(),
        theme(),
        || Gallery::new(()),
    )
}

fn click(app: &mut HeadlessApp, key: &str) {
    app.click(&Query::key(key)).expect(key);
}

#[test]
fn the_token_set_styles_the_library() {
    let theme = theme();
    assert_eq!(
        theme.tokens().get("color-brand-teal").map(ToString::to_string).as_deref(),
        Some("#00766c")
    );
    assert!(theme.tokens().get("color-border").is_some(), "a role the set omits is the library's");
    assert!(theme.host_roles().iter().any(|(name, _)| name.as_ref() == "color-accent"));
}

#[test]
fn every_page_realizes_from_the_library() {
    let mut app = launch();
    assert_golden!("gallery-overview", app.golden());
    assert!(app.find(&Query::key("plot")).is_ok(), "the chart");

    click(&mut app, "destination-1");
    assert!(app.find(&Query::key("s1-5")).is_ok(), "the sectioned grid");
    assert_golden!("gallery-data", app.golden());

    click(&mut app, "destination-2");
    assert_golden!("gallery-settings", app.golden());
    click(&mut app, "button");
    assert!(app.find(&Query::text("Saved Ada")).is_ok(), "{}", app.golden());
}
