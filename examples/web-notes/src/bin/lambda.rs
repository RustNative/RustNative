//! The notes application as a function (AWS Lambda's runtime API):
//! `rustnative serve lambda target/debug/web-notes-lambda` runs it locally.
//! Its data is the server's data service (`NOTES_DATA_URL`,
//! `NOTES_DATA_KEY`); its sessions are sealed with `NOTES_SECRET_KEY`.

fn main() -> std::process::ExitCode {
    let data = web_notes::Data::from_env();
    rustnative_server::serverless::run(web_notes::app(data, web_notes::key_from_env()))
}
