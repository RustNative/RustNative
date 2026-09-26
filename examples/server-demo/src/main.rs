//! The notes server: `cargo run -p server-demo`, then sign in at
//! <http://127.0.0.1:8080/> as `ada` / `analytical engine`.

use std::path::Path;

use rustnative_server::db::Db;
use rustnative_server::jobs::Jobs;
use server_demo::{IndexNote, app, database, settings};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (settings, sources) = settings(Path::new(env!("CARGO_MANIFEST_DIR")))?;
    let db = Db::open(&settings.database, 4)?;
    database(&db)?;
    let jobs = Jobs::new(db.clone())?.register::<IndexNote>();
    let key = if settings.secret_key.expose().len() == 64 {
        let hex = settings.secret_key.expose();
        let mut key = [0u8; 32];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)?;
        }
        key
    } else {
        eprintln!("notes: no NOTES_SECRET_KEY; sessions will not survive a restart");
        let mut key = [0u8; 32];
        key.copy_from_slice(&rustnative_server::random_token(24).into_bytes()[..32]);
        key
    };
    let application = app(&db, &jobs, key);
    println!("notes: configuration from {}", sources.join(", "));
    for line in application.report() {
        println!("notes: {line}");
    }
    let listener = tokio::net::TcpListener::bind(&settings.address).await?;
    println!("notes: listening on http://{}", listener.local_addr()?);
    let worker = tokio::spawn({
        let jobs = jobs.clone();
        async move { jobs.run(std::time::Duration::from_secs(1), std::future::pending()).await }
    });
    application
        .into_service()
        .serve(listener, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    worker.abort();
    Ok(())
}
