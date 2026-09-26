//! The sync service (`PLAN.md` Milestone 55): local-first writes offline,
//! convergence after reconnecting, each conflict policy, partial
//! replication, schema versioning, server push, and presence.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed expectation in an integration test is the test failing"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustnative_sync::{
    Channel, Clock, ConflictPolicy, Filter, InMemory, LocalHub, Presence, Record, SyncError,
    SyncServer, SyncedCollection,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Note {
    id: String,
    owner: String,
    text: String,
}

impl Record for Note {
    const COLLECTION: &'static str = "notes";
    fn id(&self) -> String {
        self.id.clone()
    }
}

fn note(id: &str, owner: &str, text: &str) -> Note {
    Note { id: id.into(), owner: owner.into(), text: text.into() }
}

fn server(policy: ConflictPolicy) -> Arc<Mutex<SyncServer>> {
    Arc::new(Mutex::new(SyncServer::new().collection::<Note>(policy, 1)))
}

#[tokio::test]
async fn two_offline_devices_converge_last_writer_wins() {
    let server = server(ConflictPolicy::LastWriterWins);
    let (laptop, phone) = (InMemory::new(Arc::clone(&server)), InMemory::new(Arc::clone(&server)));
    let mut on_laptop = SyncedCollection::<Note>::new(Arc::new(Clock::new(1)));
    let mut on_phone = SyncedCollection::<Note>::new(Arc::new(Clock::new(2)));
    on_laptop.put(&note("n1", "ada", "Buy milk")).unwrap();
    on_laptop.sync(&laptop).await.unwrap();
    on_phone.sync(&phone).await.unwrap();

    laptop.set_online(false);
    phone.set_online(false);
    on_laptop.put(&note("n1", "ada", "Buy oat milk")).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    on_phone.put(&note("n1", "ada", "Buy oat milk and bread")).unwrap();
    on_phone.put(&note("n2", "ada", "Call Grace")).unwrap();
    assert!(matches!(on_laptop.sync(&laptop).await, Err(SyncError::Unreachable(_))));
    assert_eq!(on_laptop.get("n1").unwrap().text, "Buy oat milk", "reads are local while offline");
    assert_eq!(on_laptop.pending(), 1, "nothing lost");

    laptop.set_online(true);
    phone.set_online(true);
    on_laptop.sync(&laptop).await.unwrap();
    on_phone.sync(&phone).await.unwrap();
    on_laptop.sync(&laptop).await.unwrap();
    assert_eq!(on_laptop.list(), on_phone.list(), "converged");
    assert_eq!(on_laptop.get("n1").unwrap().text, "Buy oat milk and bread", "the later write");
    assert_eq!(on_laptop.list().len(), 2);
}

#[tokio::test]
async fn server_authority_refuses_an_outdated_write() {
    let server = server(ConflictPolicy::ServerAuthority);
    let transport = InMemory::new(Arc::clone(&server));
    let mut first = SyncedCollection::<Note>::new(Arc::new(Clock::new(1)));
    let mut second = SyncedCollection::<Note>::new(Arc::new(Clock::new(2)));
    first.put(&note("n1", "ada", "v1")).unwrap();
    first.sync(&transport).await.unwrap();
    second.sync(&transport).await.unwrap();
    first.put(&note("n1", "ada", "first's edit")).unwrap();
    first.sync(&transport).await.unwrap();
    second.put(&note("n1", "ada", "second's edit, from v1")).unwrap();
    let report = second.sync(&transport).await.unwrap();
    assert_eq!(report.refused, 1);
    assert_eq!(second.get("n1").unwrap().text, "first's edit", "the server's copy");
}

#[tokio::test]
async fn a_merge_policy_combines_both_writes() {
    let merge: ConflictPolicy =
        ConflictPolicy::Merge(Arc::new(|server: &Value, client: &Value| {
            let mut merged = server.clone();
            let text = format!(
                "{} / {}",
                server["text"].as_str().unwrap_or(""),
                client["text"].as_str().unwrap_or("")
            );
            merged["text"] = json!(text);
            merged
        }));
    let server = server(merge);
    let transport = InMemory::new(Arc::clone(&server));
    let mut first = SyncedCollection::<Note>::new(Arc::new(Clock::new(1)));
    let mut second = SyncedCollection::<Note>::new(Arc::new(Clock::new(2)));
    first.put(&note("n1", "ada", "eggs")).unwrap();
    second.put(&note("n1", "ada", "flour")).unwrap();
    first.sync(&transport).await.unwrap();
    second.sync(&transport).await.unwrap();
    first.sync(&transport).await.unwrap();
    assert_eq!(first.get("n1").unwrap().text, "eggs / flour");
    assert_eq!(first.list(), second.list());
}

#[tokio::test]
async fn a_replica_holds_only_what_its_filter_selects() {
    let server = server(ConflictPolicy::LastWriterWins);
    let transport = InMemory::new(Arc::clone(&server));
    let mut everyone = SyncedCollection::<Note>::new(Arc::new(Clock::new(1)));
    everyone.put(&note("n1", "ada", "Ada's")).unwrap();
    everyone.put(&note("n2", "grace", "Grace's")).unwrap();
    everyone.sync(&transport).await.unwrap();
    let mut grace = SyncedCollection::<Note>::new(Arc::new(Clock::new(2)))
        .filtered(Filter { field: "owner".into(), value: json!("grace") });
    grace.sync(&transport).await.unwrap();
    assert_eq!(grace.list(), vec![note("n2", "grace", "Grace's")]);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct NoteV2 {
    id: String,
    owner: String,
    title: String,
    pinned: bool,
}

impl Record for NoteV2 {
    const COLLECTION: &'static str = "notes";
    const VERSION: u32 = 2;
    fn id(&self) -> String {
        self.id.clone()
    }
    fn upgrade(from: u32, mut value: Value) -> Result<Value, String> {
        if from == 1 {
            value["title"] = value["text"].take();
            value["pinned"] = json!(false);
            Ok(value)
        } else {
            Err(format!("no upgrade from {from}"))
        }
    }
}

#[tokio::test]
async fn clients_at_different_schema_versions_share_data() {
    // The server runs version 2 and still reads version 1 clients' writes.
    let server = Arc::new(Mutex::new(
        SyncServer::new().collection::<NoteV2>(ConflictPolicy::LastWriterWins, 1),
    ));
    let transport = InMemory::new(Arc::clone(&server));
    let mut old_client = SyncedCollection::<Note>::new(Arc::new(Clock::new(1)));
    old_client.put(&note("n1", "ada", "From the old app")).unwrap();
    old_client.sync(&transport).await.unwrap();
    let mut new_client = SyncedCollection::<NoteV2>::new(Arc::new(Clock::new(2)));
    new_client.sync(&transport).await.unwrap();
    assert_eq!(new_client.get("n1").unwrap().title, "From the old app", "upgraded on the way in");

    // A server that no longer serves version 1 tells the old client so.
    let strict = Arc::new(Mutex::new(
        SyncServer::new().collection::<NoteV2>(ConflictPolicy::LastWriterWins, 2),
    ));
    let result =
        SyncedCollection::<Note>::new(Arc::new(Clock::new(3))).sync(&InMemory::new(strict)).await;
    assert_eq!(result, Err(SyncError::NeedsUpgrade(2)));
}

#[tokio::test]
async fn the_server_pushes_a_signal_when_something_changes() {
    let server = server(ConflictPolicy::LastWriterWins);
    let mut changed = server.lock().unwrap().subscribe();
    let transport = InMemory::new(Arc::clone(&server));
    let mut writer = SyncedCollection::<Note>::new(Arc::new(Clock::new(1)));
    writer.put(&note("n1", "ada", "hi")).unwrap();
    writer.sync(&transport).await.unwrap();
    assert_eq!(changed.recv().await.unwrap(), 1, "a waiting replica wakes and syncs");
}

#[tokio::test]
async fn presence_follows_heartbeats_and_channels_deliver() {
    let hub = LocalHub::new(Duration::from_millis(80));
    let mut room = hub.subscribe("room");
    assert_eq!(hub.publish("room", json!("hello")), 1);
    assert_eq!(room.recv().await.unwrap(), json!("hello"));

    hub.join("doc-1", "ada", json!({ "cursor": 3 }));
    hub.join("doc-1", "grace", json!({ "cursor": 9 }));
    assert_eq!(hub.members("doc-1").len(), 2);
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        hub.heartbeat("doc-1", "ada");
    }
    let members = hub.members("doc-1");
    assert_eq!(members.keys().collect::<Vec<_>>(), ["ada"], "Grace stopped heartbeating");
    hub.leave("doc-1", "ada");
    assert!(hub.members("doc-1").is_empty());
}
