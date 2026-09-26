use rustnative_core::rsx;

fn main() {
    let _ = rsx! { <Column key="root" paddng={rustnative_core::EdgeInsets::all(4)}></Column> };
}
