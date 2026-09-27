//! The collaborative session on the local actor system: twenty concurrent
//! appends lose nothing, the alarm fires once, and a new instance after
//! eviction finds the document as the last one left it. The same session
//! on the edge adapter is `crates/rustnative/tests/serverless.rs`.

#![allow(clippy::unwrap_used, missing_docs, reason = "tests")]

use std::time::Duration;

use edge_actors::{Document, Edit};
use rustnative_durable::LocalActorSystem;
use rustnative_server::db::Db;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_serializes_and_persists_in_a_process() {
    let db = Db::memory("edge-actors", 2).unwrap();
    let system = LocalActorSystem::<Document>::new(db, Duration::from_millis(100)).unwrap();
    system.ask("doc-1", Edit::Join("ada".into())).await.unwrap().unwrap();
    system.ask("doc-1", Edit::Join("grace".into())).await.unwrap().unwrap();
    let writers = (0..20).map(|index| {
        let system = system.clone();
        let who = if index % 2 == 0 { "ada" } else { "grace" };
        tokio::spawn(async move {
            system.ask("doc-1", Edit::Append(who.into(), format!("line {index}"))).await
        })
    });
    for writer in writers {
        writer.await.unwrap().unwrap().unwrap();
    }
    let read = system.ask("doc-1", Edit::Read).await.unwrap().unwrap();
    assert_eq!(read.text.lines().count(), 20, "no lost update");
    assert_eq!(read.editors, ["ada", "grace"]);

    assert_eq!(system.fire_alarms().unwrap(), 1);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(system.running(), 0, "evicted when idle");
    let again = system.ask("doc-1", Edit::Read).await.unwrap().unwrap();
    assert_eq!(again.text, read.text, "a new instance finds its storage");
    assert_eq!(again.reminders, 1, "the alarm fired once");
}
