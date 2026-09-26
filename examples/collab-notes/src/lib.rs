//! Collaborative notes (`PLAN.md` Milestone 55's first "done when"): a
//! shared document edited on two devices, offline, converging when they
//! reconnect.
//!
//! The document's text is a replicated sequence ([`Rga`]); the server's
//! conflict policy for documents merges the two sequences, so concurrent
//! edits interleave instead of one overwriting the other. Edits are local
//! first; [`Editor`] syncs when asked and whenever the server signals a
//! change.

use std::sync::Arc;

use rustnative_core::{Component, ComponentContext, Event, Node, NodeId};
use rustnative_sync::{
    Clock, ConflictPolicy, Crdt, Record, Rga, SyncError, SyncReport, SyncTransport,
    SyncedCollection,
};
use serde::{Deserialize, Serialize};

/// A shared document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    /// Its id.
    pub id: String,
    /// Its text, as a replicated sequence.
    pub text: Rga<char>,
}

impl Record for Doc {
    const COLLECTION: &'static str = "docs";
    fn id(&self) -> String {
        self.id.clone()
    }
}

/// The documents' conflict policy: merge the two texts.
#[must_use]
pub fn merge_policy() -> ConflictPolicy {
    ConflictPolicy::Merge(Arc::new(|server, client| {
        match (
            serde_json::from_value::<Doc>(server.clone()),
            serde_json::from_value::<Doc>(client.clone()),
        ) {
            (Ok(mut merged), Ok(theirs)) => {
                merged.text.merge(&theirs.text);
                serde_json::to_value(merged).unwrap_or_else(|_| server.clone())
            }
            _ => server.clone(),
        }
    }))
}

/// Applies an edit from `before` to `after` to `text`, as the fewest
/// deletions and insertions between their common prefix and suffix.
pub fn apply_edit(text: &mut Rga<char>, before: &str, after: &str, clock: &Clock) {
    let old: Vec<char> = before.chars().collect();
    let new: Vec<char> = after.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    for _ in prefix..old.len() - suffix {
        text.delete(prefix);
    }
    for (offset, character) in new[prefix..new.len() - suffix].iter().enumerate() {
        text.insert(prefix + offset, *character, clock.now());
    }
}

/// What finished.
#[derive(Debug)]
pub enum Done {
    /// A sync, with its outcome.
    Synced(Result<SyncReport, SyncError>),
}

/// The editor's setup.
#[derive(Clone)]
pub struct EditorProps {
    /// This device's replica id.
    pub replica: u64,
    /// The document.
    pub doc: String,
    /// The way to the server.
    pub transport: Arc<dyn SyncTransport>,
}

impl PartialEq for EditorProps {
    fn eq(&self, other: &Self) -> bool {
        self.replica == other.replica
            && self.doc == other.doc
            && Arc::ptr_eq(&self.transport, &other.transport)
    }
}

/// The collaborative editor.
pub struct Editor {
    props: EditorProps,
    clock: Arc<Clock>,
    docs: Arc<tokio::sync::Mutex<SyncedCollection<Doc>>>,
    text: String,
    status: String,
    sync_requested: bool,
}

impl Editor {
    fn current(&self) -> Doc {
        self.docs
            .try_lock()
            .ok()
            .and_then(|docs| docs.get(&self.props.doc))
            .unwrap_or_else(|| Doc { id: self.props.doc.clone(), text: Rga::default() })
    }
}

impl Component for Editor {
    type Props = EditorProps;
    type Message = Done;

    fn new(props: EditorProps) -> Self {
        let clock = Arc::new(Clock::new(props.replica));
        let docs = Arc::new(tokio::sync::Mutex::new(SyncedCollection::new(Arc::clone(&clock))));
        Self {
            props,
            clock,
            docs,
            text: String::new(),
            status: "Not synced yet".into(),
            sync_requested: true,
        }
    }
    fn props(&self) -> &EditorProps {
        &self.props
    }
    fn set_props(&mut self, props: EditorProps) {
        self.props = props;
    }
    fn view(&self) -> Node {
        Node::column("editor", [])
    }

    fn update(&mut self, event: Event) {
        match event {
            Event::TextChanged { target, value } if target == NodeId::from_key("text") => {
                let mut doc = self.current();
                apply_edit(&mut doc.text, &self.text, &value, &self.clock);
                if let Ok(mut docs) = self.docs.try_lock() {
                    if let Err(error) = docs.put(&doc) {
                        self.status = format!("Not saved: {error}");
                    }
                }
                self.text = value;
            }
            Event::Click { target } if target == NodeId::from_key("sync") => {
                self.sync_requested = true;
            }
            _ => {}
        }
    }

    fn message(&mut self, done: Done) {
        let Done::Synced(result) = done;
        self.status = match result {
            Ok(report) => format!("Synced: {} sent, {} received", report.sent, report.received),
            Err(error) => format!("Offline — edits are kept ({error})"),
        };
        self.text = self.current().text.text();
    }

    fn render(&mut self, context: &mut ComponentContext<'_, Done>) -> Node {
        if std::mem::take(&mut self.sync_requested) {
            let (docs, transport) = (Arc::clone(&self.docs), Arc::clone(&self.props.transport));
            context.spawn(async move {
                let result = docs.lock().await.sync(&*transport).await;
                Done::Synced(result)
            });
        }
        let pending = self.docs.try_lock().map_or(0, |docs| docs.pending());
        Node::column(
            "editor",
            [
                Node::text_input("text", self.text.clone()),
                Node::row(
                    "bar",
                    [Node::button("sync", "Sync"), Node::label("status", self.status.clone())],
                ),
                Node::label("pending", format!("{pending} unsent")),
            ],
        )
    }
}
