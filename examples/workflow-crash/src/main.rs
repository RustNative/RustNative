//! `workflow-crash <database>` runs, or resumes, order 42's workflow.
//! With `CRASH=after:reserve` or `CRASH=inside:charge`, the process kills
//! itself at that point. Run it again without `CRASH` and the order
//! completes.

use rustnative_durable::{LocalEngine, Status, WorkflowEngine};
use rustnative_server::db::Db;
use workflow_crash::{Fulfil, Order, schema};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "orders.db".into());
    let db = Db::open(&path, 1)?;
    schema(&db.get())?;
    let engine = LocalEngine::new(db)?.register(Fulfil);
    engine.start::<Fulfil>("order-42", &Order { id: "42".into(), amount: 1999 })?;
    engine.run_until_idle().await;
    match engine.status("order-42") {
        Some(Status::Completed(output)) => println!("{}", output.trim_matches('"')),
        other => println!("not finished: {other:?}"),
    }
    Ok(())
}
