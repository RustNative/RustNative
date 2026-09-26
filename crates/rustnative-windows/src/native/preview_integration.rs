//! The preview catalogue on the real backend (`PLAN.md` Milestone 43):
//! previews realized as native controls, the configuration toolbar
//! restyling and mirroring them.

use rustnative_core::preview::{Catalogue, Preview, PreviewMatrix};
use rustnative_core::{Application, Node, Size, Window, WindowId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, WS_EX_LAYOUTRTL,
};

use super::harness::NativeHarness;

#[test]
fn native_catalogue_shows_previews_and_follows_the_toolbar() {
    let previews = vec![
        Preview::new("greeting", || Node::label("hello", "Hello")),
        Preview::new("form", || Node::button("save", "Save")).with_matrix(PreviewMatrix::full()),
    ];
    let mut application = Application::new(
        Catalogue::open(previews, "form"),
        Window::new("Previews", Size::new(640, 400)),
    );
    // SAFETY: `application` is declared first, so it outlives the harness.
    let mut harness = unsafe { NativeHarness::attach(&mut application) };
    assert!(harness.control(WindowId::PRIMARY, "save").is_some(), "opened at the named preview");
    harness.click(WindowId::PRIMARY, "preview-0");
    assert!(harness.control(WindowId::PRIMARY, "hello").is_some());

    // Mirroring the stage reaches the native objects.
    harness.click(WindowId::PRIMARY, "direction");
    let frame = harness.expect_control(WindowId::PRIMARY, "preview-frame");
    // SAFETY: a live window of the harness.
    let style = unsafe { GetWindowLongPtrW(frame, GWL_EXSTYLE) };
    assert_ne!(
        style & isize::try_from(WS_EX_LAYOUTRTL).unwrap(),
        0,
        "the preview frame is mirrored"
    );
}
