//! The long-lived server: `cargo run -p web-notes`, then sign in at
//! <http://127.0.0.1:8080/> as `ada` / `analytical engine`.
//!
//! `NOTES_ADDRESS` (default `127.0.0.1:8080`), `NOTES_DATABASE` (default
//! `notes.db`), `NOTES_SECRET_KEY` (64 hex digits; the session key every
//! shape shares), and `NOTES_DATA_KEY` (what the serverless shapes present
//! to `/data/*`; the data service is off without it).

use std::time::Duration;

use web_notes::local::{Local, data_service};
use web_notes::{Data, app, key_from_env};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("NOTES_ADDRESS").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let database = std::env::var("NOTES_DATABASE").unwrap_or_else(|_| "notes.db".into());
    let local = Local::open(&database)?;
    let mut application = app(Data::Local(local.clone()), key_from_env());
    if let Ok(key) = std::env::var("NOTES_DATA_KEY") {
        application = data_service(application, &local, key);
    }
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("notes: listening on http://{}", listener.local_addr()?);
    let worker = tokio::spawn({
        let jobs = local.jobs.clone();
        async move { jobs.run(Duration::from_millis(200), std::future::pending()).await }
    });
    application.into_service().serve(listener, std::future::pending()).await?;
    worker.abort();
    Ok(())
}
