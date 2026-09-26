//! The markup and style diagnostics, held by a compile-failure suite
//! rather than by inspection (`PLAN.md` Milestones 53 and 58): each case
//! must fail to compile with the error its `.stderr` records, spanned to
//! what the developer wrote. `.rsx`-file diagnostics, which are the same
//! errors reported through the source map, and `app.css` diagnostics are
//! covered by `rustnative`'s CLI tests.

#[test]
fn markup_diagnostics() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}

#[test]
fn style_diagnostics() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/style_ui/*.rs");
    // A property the Windows backend cannot realize is an error when
    // building for Windows (and only then).
    if cfg!(windows) {
        cases.compile_fail("tests/style_ui_windows/*.rs");
    }
}
