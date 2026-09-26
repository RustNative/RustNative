use rustnative_core::rsx;

fn main() {
    let _ = rsx! { <Label key="a" text="A"><Label key="b" text="B" /></Label> };
}
