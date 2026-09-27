//! The document actor on an edge host: build with `cargo build -p
//! edge-actors --target wasm32-wasip1 --no-default-features --release`,
//! then `rustnative serve wagi
//! target/wasm32-wasip1/release/edge-actors.wasm --actors /actors/`, and
//! `POST /actors/{id}` an `Edit` as JSON.

#[cfg(target_os = "wasi")]
fn main() -> std::process::ExitCode {
    rustnative_durable::edge::serve::<edge_actors::Document>("/actors/")
}

#[cfg(not(target_os = "wasi"))]
fn main() {
    eprintln!("edge-actors is an edge module: build it for wasm32-wasip1");
}
