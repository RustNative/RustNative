use rustnative_core::rsx;

fn main() {
    let color = "bg-blue-500";
    let _ = rsx! { <Label key="l" text="x" class={color} /> };
}
