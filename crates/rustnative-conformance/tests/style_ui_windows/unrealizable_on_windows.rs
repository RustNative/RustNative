// A shadow cannot be realized by the Windows backend (its capability
// table, `rustnative_style::WINDOWS`): building for Windows fails at the
// class. This case runs where the suite runs — on Windows.
use rustnative_core::rsx;

fn main() {
    let _ = rsx! { <Column key="card" class="shadow-md"></Column> };
}
