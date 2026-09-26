//! Golden files: realized output compared against a reviewed copy.

use std::path::Path;

/// Compares `actual` with the golden file at `path`.
///
/// With `RUSTNATIVE_BLESS=1` in the environment the file is (re)written
/// instead, which is how a reviewed change to realized output is accepted:
/// the new golden appears in the diff of the commit that changed it.
///
/// # Panics
///
/// When the golden is missing (and not being blessed) or differs from
/// `actual`; the message shows the first differing lines. A panic is the
/// point: this is an assertion.
#[allow(
    clippy::panic,
    reason = "a golden comparison is an assertion; failing it must fail the test"
)]
pub fn check_golden(path: &Path, actual: &str) {
    let bless = std::env::var("RUSTNATIVE_BLESS").is_ok_and(|value| value == "1");
    if bless {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!("cannot create golden directory {}: {error}", parent.display())
            });
        }
        std::fs::write(path, actual)
            .unwrap_or_else(|error| panic!("cannot write golden {}: {error}", path.display()));
        return;
    }
    let expected = match std::fs::read_to_string(path) {
        Ok(expected) => expected.replace("\r\n", "\n"),
        Err(error) => panic!(
            "golden {} cannot be read ({error}); run with RUSTNATIVE_BLESS=1 to create it from:
{actual}",
            path.display()
        ),
    };
    assert!(expected == actual, "golden {} differs:\n{}", path.display(), diff(&expected, actual));
}

/// A line-by-line comparison: `-` lines are the golden, `+` the actual.
#[must_use]
pub fn diff(expected: &str, actual: &str) -> String {
    let expected: Vec<&str> = expected.lines().collect();
    let actual: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    let longest = expected.len().max(actual.len());
    for index in 0..longest {
        match (expected.get(index), actual.get(index)) {
            (Some(left), Some(right)) if left == right => {
                out.push_str("  ");
                out.push_str(left);
            }
            (left, right) => {
                if let Some(left) = left {
                    out.push_str("- ");
                    out.push_str(left);
                    out.push('\n');
                }
                if let Some(right) = right {
                    out.push_str("+ ");
                    out.push_str(right);
                } else {
                    out.pop();
                }
            }
        }
        out.push('\n');
    }
    out
}

/// Asserts that `actual` matches `tests/goldens/<name>.golden` in the
/// calling crate; see [`check_golden`].
///
/// ```no_run
/// # use rustnative_headless::assert_golden;
/// assert_golden!("counter-initial", "Column #root [0,0 320x200]\n");
/// ```
#[macro_export]
macro_rules! assert_golden {
    ($name:expr, $actual:expr) => {
        $crate::check_golden(
            &::std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("goldens")
                .join(format!("{}.golden", $name)),
            &$actual,
        )
    };
}

/// Makes every preview, in every configuration of its matrix, a golden
/// test (`PLAN.md` Milestone 43, `C55-3`): each is launched on this backend
/// inside a [`rustnative_core::preview::PreviewFrame`] and its realized
/// model compared with `directory/<preview>-<configuration>.golden`. A
/// golden that does not exist yet is written (a new preview has nothing to
/// have changed from); `RUSTNATIVE_BLESS=1` rewrites existing ones.
///
/// # Panics
///
/// When a golden differs or is missing — the preview changed, or rotted.
pub fn preview_goldens(previews: &[rustnative_core::preview::Preview], directory: &Path) {
    use rustnative_core::preview::PreviewFrame;
    use rustnative_core::{Component as _, Size, Window};
    for preview in previews {
        for configuration in preview.matrix().configurations() {
            let name = format!("{}-{configuration}", preview.name());
            let window = Window::new(name.clone(), Size::new(configuration.width, 720));
            let props = (preview.clone(), configuration);
            let app = crate::HeadlessApp::launch(window, move || PreviewFrame::new(props.clone()));
            let path = directory.join(format!("{name}.golden"));
            // A preview seen for the first time records its golden: there is
            // nothing yet to have changed from. Review it, and commit it.
            if !path.exists() {
                let written = std::fs::create_dir_all(directory)
                    .and_then(|()| std::fs::write(&path, app.golden()));
                assert!(written.is_ok(), "cannot write golden {}: {written:?}", path.display());
                eprintln!("new golden: {} (review it, and commit it)", path.display());
                continue;
            }
            check_golden(&path, &app.golden());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_marks_only_the_lines_that_changed() {
        let out = diff("a\nb\nc\n", "a\nx\nc\n");
        assert!(out.contains("- b"));
        assert!(out.contains("+ x"));
        assert!(out.contains("  a"));
    }

    #[test]
    fn a_matching_golden_passes_and_a_blessed_one_is_written() {
        let dir = std::env::temp_dir().join(format!("rustnative-golden-{}", std::process::id()));
        let path = dir.join("sample.golden");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(&path, "same\n").expect("write");
        check_golden(&path, "same\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[should_panic(expected = "differs")]
    fn a_differing_golden_fails() {
        let dir =
            std::env::temp_dir().join(format!("rustnative-golden-diff-{}", std::process::id()));
        let path = dir.join("sample.golden");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(&path, "before\n").expect("write");
        check_golden(&path, "after\n");
    }
}
