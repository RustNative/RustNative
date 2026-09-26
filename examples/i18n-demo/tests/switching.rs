//! The example switches locale at runtime, on the headless backend: the
//! texts are the locale's, Polish plurals take their forms, Arabic is laid
//! out right to left, and the pseudo-locale lengthens every string.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helper functions in an integration test are test code too: a failed expectation is the test failing"
)]

use i18n_demo::{Demo, messages, text};
use rustnative_core::{Component, LayoutDirection, Services, Size, Theme, Window};
use rustnative_headless::{HeadlessApp, Query};

fn launch() -> HeadlessApp {
    let services = Services::default().with_catalogues(messages::catalogues());
    HeadlessApp::launch_with(
        Window::new("i18n", Size::new(520, 360)),
        services,
        Theme::default(),
        || Demo::new(()),
    )
}

fn shown(app: &HeadlessApp, key: &str) -> String {
    app.find(&Query::key(key)).expect("the node is realized").text.clone().unwrap_or_default()
}

#[test]
fn switching_locale_changes_every_message_at_runtime() {
    let mut app = launch();
    assert_eq!(shown(&app, "count"), "You have 3 messages.");
    app.click(&Query::key("locale-pl")).unwrap();
    assert_eq!(shown(&app, "title"), "Skrzynka odbiorcza");
    assert_eq!(shown(&app, "count"), "Masz 3 wiadomości.");
    for _ in 0..2 {
        app.click(&Query::key("more")).unwrap();
    }
    assert_eq!(shown(&app, "count"), "Masz 5 wiadomości.", "few, then many");
    app.click(&Query::key("locale-en")).unwrap();
    assert_eq!(shown(&app, "count"), "You have 5 messages.", "the state survives the switch");
}

#[test]
fn arabic_takes_its_six_plural_forms_and_mirrors() {
    let mut app = launch();
    app.click(&Query::key("locale-ar")).unwrap();
    assert_eq!(shown(&app, "count"), "لديك 3 رسائل.", "few");
    for _ in 0..3 {
        app.click(&Query::key("fewer")).unwrap();
    }
    assert_eq!(shown(&app, "count"), "صندوق الوارد فارغ.", "zero");
    assert_eq!(shown(&app, "invited"), "دعتك Ada إلى فريقها.", "the feminine form");
    let snapshot = app.realized().snapshot();
    let screen =
        snapshot.nodes().find(|node| node.id.local_key().as_deref() == Some("screen")).unwrap();
    assert_eq!(screen.layout.direction, Some(LayoutDirection::Rtl));
    // Mirrored: the first action sits right of the second.
    let more = app.find(&Query::key("more")).unwrap().window_rect;
    let fewer = app.find(&Query::key("fewer")).unwrap().window_rect;
    assert!(more.x > fewer.x, "more {more:?} fewer {fewer:?}");
}

#[test]
fn the_pseudo_locale_lengthens_every_message() {
    let mut app = launch();
    let english = shown(&app, "invited");
    app.click(&Query::key("locale-en-XA")).unwrap();
    let pseudo = shown(&app, "invited");
    assert!(pseudo.chars().count() > english.chars().count(), "{pseudo}");
    assert_eq!(text(&messages::title(), "pl"), "Skrzynka odbiorcza");
}
