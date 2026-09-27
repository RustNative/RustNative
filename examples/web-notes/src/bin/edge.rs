//! The notes application as an edge module (WAGI over `wasm32-wasip1`):
//! `cargo build -p web-notes --bin web-notes-edge --target wasm32-wasip1
//! --no-default-features`, then `rustnative serve wagi
//! target/wasm32-wasip1/debug/web-notes-edge.wasm --allow-http
//! 127.0.0.1:8080`. One request per run; its data is the server's data
//! service, reached through the host.

fn main() -> std::process::ExitCode {
    let data = web_notes::Data::from_env();
    rustnative_server::serverless::run(web_notes::app(data, web_notes::key_from_env()))
}
